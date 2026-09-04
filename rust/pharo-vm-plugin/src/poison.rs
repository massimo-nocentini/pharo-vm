//! Fail-fast after a panic that tore this module's shared state.
//!
//! Plugin cdylibs are built with `panic = "unwind"` (see
//! `rust/plugins/Cargo.toml`), so the [`std::panic::catch_unwind`] the SDK
//! wraps every primitive body in is live: a panic becomes a primitive failure
//! and the image runs its Smalltalk fallback instead of the VM dying under
//! the user.
//!
//! That is the right answer for a panic in a *computation*. It is the wrong
//! answer for a panic *mid-mutation of shared state*: [`crate::handles::Registry`]
//! holds its mutex across the caller's closure, so a panic in there leaves a
//! slot half-written, the unwind carries on, and the next primitive would
//! proceed on a torn invariant. Continuing on a broken invariant is strictly
//! worse than aborting.
//!
//! Two mechanisms answer that, and each covers what the other cannot.
//!
//! * **std's own mutex poison** is exact and per-registry: a `Mutex` is
//!   poisoned precisely when a guard is dropped during an unwind.
//!   `Registry::lock` honours it rather than swallowing it, so *that* registry
//!   refuses every later operation with [`crate::PrimErr::Unsupported`].
//! * **The module flag here** covers what is not behind a registry mutex, and
//!   lets `run_primitive` refuse *before* it knows which state a body would
//!   have touched. It is set from a [`std::panic::set_hook`] rather than from
//!   the error arm of a lock, because the hook runs **before** the unwind,
//!   while the guard is still held -- which is the only moment at which "this
//!   panic is tearing something" can be told from "this panic tore nothing".
//!
//! # Scope
//!
//! The flag is per-dlopened-cdylib, which here is the same thing as
//! per-plugin-module: each cdylib statically links its own libstd and its own
//! copy of this crate, so each has its own `HOOK` and its own [`POISONED`],
//! with no interposition between them. One plugin's torn state must not
//! disable the other fifteen, and that is exactly what this granularity gives.
//!
//! # Reach, honestly stated
//!
//! The gate is conservative in one direction: a panic inside a *read-only*
//! `Registry::with` closure poisons too, because the guard is held and neither
//! std's poison nor a depth counter can tell a read from a write. Nothing is
//! lost by that -- std poisons the mutex on the same unwind either way, so the
//! registry was already unusable -- but it is the reason a plugin should not
//! wrap a `Section` around more than the mutation it protects.
//!
//! [`Section`] is what tells the hook that a panic is tearing something.
//! [`Registry`](crate::handles::Registry) opens one for every lock it takes,
//! and so does [`lock`], which is how a plugin's own `static Mutex` gets the
//! same treatment: it refuses a mutex std has already poisoned and opens a
//! `Section` for the life of the guard it hands back. Every process-global
//! mutex a primitive can reach goes through one of those two.
//!
//! The rest of the plugin tree's shared mutable state is accounted for as
//! follows, because "it is behind a mutex" is not by itself an answer:
//!
//! * `b2d-plugin`'s `with_globals` is an `UnsafeCell` asserted `Sync` on the
//!   single-interpreter-thread contract, exactly as the C left those globals
//!   in file scope. There is no mutex for std to poison, so [`Section::enter`]
//!   applied directly is the whole of the fail-fast story there.
//! * The dlopen function tables in cairo/pango/sdl3 need nothing. They are
//!   `OnceLock`s written once by an initialiser and read-only afterwards, and
//!   `OnceLock::get_or_init` leaves the cell *empty* when the initialiser
//!   panics -- the next call re-runs it rather than observing a half-built
//!   table. There is no torn state to protect against.
//! * `unix-os-process-plugin`'s `send_signal_to_pids` is an `atexit` hook and
//!   uses `try_lock` on purpose: blocking there would deadlock a process that
//!   is exiting mid-primitive. `try_lock` answers `Err` on a poisoned mutex
//!   too, so it skips a torn list for free.
//! * `#[cfg(test)]` serialisation locks (`socket-plugin`'s `net_lock`,
//!   `unix-os-process-plugin`'s `SIGNAL_TEST_LOCK`, the `tests/` guards in
//!   pango and sdl3) recover their own poison deliberately: one failing test
//!   must not cascade into every test that shares the lock, and no image ever
//!   reaches them.
//!
//! # Afterwards
//!
//! A poisoned module fails every primitive for the life of the process, with
//! no reset. Nothing in the image knows how to re-establish the invariant that
//! was torn -- it is a half-inserted `cairo_t *` in a slot table the image
//! cannot see -- so a reset would only hand back handles that resolve to
//! garbage. `shutdownModule` still runs, so the VM can unload the module
//! cleanly; it simply must not free anything, which is why
//! [`crate::handles::Registry::drain`] answers nothing once poisoned.

use std::cell::Cell;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, Once};

use crate::error::{PrimErr, PrimResult};

/// Set once this module's shared state has been torn by a panic.
///
/// Read by `run_primitive` before every primitive body. Never cleared: see
/// *Afterwards* in the module docs.
static POISONED: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// How many critical sections *this thread* has open right now.
    ///
    /// Thread-local rather than a process-wide counter: a guard held on
    /// another thread is not torn by this thread's panic, so a shared counter
    /// would poison the module on a false positive.
    static DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// The current thread's critical-section depth, infallibly.
///
/// `try_with(..).unwrap_or(0)` rather than `with`: this is called from a panic
/// hook, and a panic inside a panic hook aborts. `Cell<u32>` has no
/// destructor, so `with` would not in fact fail during TLS teardown -- but the
/// hook is not the place to depend on that.
fn depth() -> u32 {
    DEPTH.try_with(Cell::get).unwrap_or(0)
}

/// Has a panic torn this module's shared state?
///
/// Once true, always true.
#[must_use]
pub fn is_poisoned() -> bool {
    POISONED.load(Ordering::Relaxed)
}

/// Open for as long as state this module owns is mid-mutation.
///
/// A panic while one is open poisons the module; a panic while none is open is
/// an ordinary primitive failure. `Registry` opens one for the life of every
/// lock it takes, and so does [`lock`] -- so a plugin's own `static Mutex`
/// wants [`lock`], not this. Reach for `Section::enter` only where there is no
/// mutex to lock, as in `b2d-plugin`'s `UnsafeCell` globals:
///
/// ```ignore
/// let _section = Section::enter();
/// // ... mutate the state no Registry and no Mutex owns ...
/// ```
pub struct Section(());

impl Section {
    /// Declares this thread to be mid-mutation until the value is dropped.
    #[must_use]
    pub fn enter() -> Self {
        DEPTH.with(|d| d.set(d.get().saturating_add(1)));
        Section(())
    }
}

impl Drop for Section {
    fn drop(&mut self) {
        // Runs on the unwind path too, which is what keeps the counter from
        // leaking after a panic the hook has already accounted for.
        DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

/// A held lock, plus the declaration that this thread is mid-mutation.
///
/// Derefs to the guarded value, so it substitutes for a [`MutexGuard`] at a
/// call site. The [`Section`] is dropped with it -- on the unwind path too,
/// which is why the depth it maintains does not leak.
pub struct Guarded<'a, T: ?Sized> {
    inner: MutexGuard<'a, T>,
    _section: Section,
}

impl<T: ?Sized> Deref for Guarded<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T: ?Sized> DerefMut for Guarded<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

/// Locks `mutex` for a critical section, refusing once a panic has torn what
/// it protects.
///
/// This is the whole poison story for a plugin's own `static Mutex`, and the
/// two halves are separate:
///
/// * **Refusing a poisoned mutex** ([`PrimErr::Unsupported`]) is what stops
///   the *next* caller reading state a panic left half-written. Recovering it
///   with `PoisonError::into_inner` instead is what this exists to replace:
///   under `panic = "unwind"` that turns "die on a broken invariant" into
///   "continue on one".
/// * **Opening a [`Section`]** is what stops the caller after that from even
///   reaching a second, unpoisoned mutex in the same module: the panic hook
///   sees the open section and poisons the whole module, so `run_primitive`
///   fails before any body runs.
///
/// ```ignore
/// static CACHE: Mutex<Cache> = Mutex::new(Cache::new());
///
/// fn cache() -> PrimResult<Guarded<'static, Cache>> {
///     poison::lock(&CACHE)
/// }
/// ```
pub fn lock<T: ?Sized>(mutex: &Mutex<T>) -> PrimResult<Guarded<'_, T>> {
    if mutex.is_poisoned() {
        return Err(PrimErr::Unsupported);
    }
    match mutex.lock() {
        Ok(inner) => Ok(Guarded {
            inner,
            _section: Section::enter(),
        }),
        // Poisoned between the check above and here.
        Err(_) => Err(PrimErr::Unsupported),
    }
}

/// Chains a hook ahead of the default one that poisons this module when a
/// panic happens with a [`Section`] open.
///
/// Called from `setInterpreter`, which is the one seam every plugin has and
/// every plugin reaches first. The `pharo_plugin!` macro emits it from its
/// `@common` arm, so all 20 invocations in this tree export it, and
/// `callInitializersIn` in `src/common/sqNamedPrims.c` looks it up, rejects
/// the library when it is missing, and only then calls `initialiseModule`.
///
/// `initialiseModule` is the obvious alternative and is the wrong one -- not
/// because init hooks are rare (12 of the 20 invocations declare `init =`, so
/// most plugins do have one), but because the macro emits it only from its
/// `@init` arm, so the 8 that do not declare one would install no hook at all
/// and would be left with the pre-`unwind` behaviour this module exists to
/// replace.
///
/// Guarded by a [`Once`], so a repeat call is harmless.
pub fn install_panic_hook(module: &'static str) {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // Before the unwind, so any guard is still held and DEPTH still
            // says whether this panic is tearing anything.
            if depth() > 0 && !POISONED.swap(true, Ordering::SeqCst) {
                eprintln!(
                    "{module}: panicked with shared state mid-mutation; \
                     this module's primitives will now fail."
                );
            }
            previous(info);
        }));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_counts_nested_sections_and_unwinds_back_to_zero() {
        assert_eq!(depth(), 0);
        let outer = Section::enter();
        assert_eq!(depth(), 1);
        {
            let _inner = Section::enter();
            assert_eq!(depth(), 2);
        }
        assert_eq!(depth(), 1);
        drop(outer);
        assert_eq!(depth(), 0);
    }

    #[test]
    fn a_section_closes_as_a_panic_passes_through_it() {
        // The property the hook depends on from the other side: `Drop` runs
        // during the unwind, so a panic does not leak depth onto the thread.
        let unwound = std::panic::catch_unwind(|| {
            let _section = Section::enter();
            panic!("tearing something");
        });
        assert!(unwound.is_err());
        assert_eq!(depth(), 0);
    }

    #[test]
    fn depth_is_per_thread() {
        let _section = Section::enter();
        assert_eq!(depth(), 1);
        let seen = std::thread::spawn(depth).join().unwrap_or(u32::MAX);
        assert_eq!(seen, 0, "another thread's mutation is not this thread's");
    }
}
