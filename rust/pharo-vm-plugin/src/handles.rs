//! Integer handles for resources a plugin owns.
//!
//! A plugin that wraps a foreign library holds things the image has no
//! representation for: a `cairo_t *`, an `SDL_Window *`, a socket. The
//! temptation is to hand the pointer over as an integer and take it back on
//! the next call. That is what the image-side FFI does, and it is why a stale
//! Athens context crashes the VM instead of failing a primitive: nothing on
//! the way in can tell a live pointer from a freed one.
//!
//! A [`Registry`] keeps the resource on the Rust side and gives the image an
//! opaque integer instead. The integer carries a slot index *and* a
//! generation counter, so a handle to a destroyed resource does not silently
//! resolve to whatever took its slot -- it fails with
//! [`PrimErr::NotFound`], and the image runs its fallback code.
//!
//! ```ignore
//! static CONTEXTS: Registry<Context> = Registry::new();
//!
//! let handle = CONTEXTS.insert(Context::new()?)?;   // hand this to the image
//! CONTEXTS.with(handle, |ctx| ctx.paint())?;        // and take it back later
//! drop(CONTEXTS.remove(handle)?);                   // destroyed exactly once
//! ```
//!
//! This is the same defence SurfacePlugin's surface IDs provide, generalised
//! and with the stale-ID hole closed: there, a reused slot resolves to its new
//! occupant.

use std::sync::{Mutex, PoisonError};

use crate::error::{PrimErr, PrimResult};
use crate::proxy::sqInt;

/// Bits of a handle spent on the slot index.
///
/// The rest carry the generation, and the whole handle must stay inside a
/// SmallInteger so the image never has to box one: 60 value bits in a 64-bit
/// image, 30 in a 32-bit image.
const INDEX_BITS: u32 = if core::mem::size_of::<sqInt>() == 8 {
    32
} else {
    14
};

/// Highest generation a slot can reach before it is retired.
///
/// A retired slot is never reused, so the counter cannot wrap and make an
/// ancient handle valid again. Reaching this needs 2^28 destroy/create cycles
/// on one slot; the cost of retiring is one leaked `Vec` entry.
const MAX_GENERATION: u32 = if core::mem::size_of::<sqInt>() == 8 {
    (1 << 28) - 1
} else {
    (1 << 15) - 1
};

const INDEX_MASK: sqInt = (1 << INDEX_BITS) - 1;

/// A table of resources the image refers to by integer.
///
/// Intended to live in a `static`. `T` must be `Send` because the registry is
/// shared: primitives run on the interpreter thread, but Rust has to be told
/// that, and a raw pointer type will need a documented `unsafe impl Send`.
pub struct Registry<T> {
    slots: Mutex<Vec<Slot<T>>>,
}

struct Slot<T> {
    /// Bumped every time the slot is refilled. Starts at 1, so a valid handle
    /// is never 0 and `nil`-shaped zero never resolves.
    generation: u32,
    value: Option<T>,
}

impl<T> Default for Registry<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Registry<T> {
    /// An empty registry, constructible in a `static`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: Mutex::new(Vec::new()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Slot<T>>> {
        // A panic inside a primitive is caught and turned into a failure, so a
        // poisoned registry is not a reason to keep failing forever: the data
        // is a plain Vec of slots and stays consistent.
        self.slots.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Stores `value` and answers the handle the image should hold.
    ///
    /// Fails with [`PrimErr::LimitExceeded`] if no slot can be allocated,
    /// which needs more than 2^32 live resources.
    pub fn insert(&self, value: T) -> PrimResult<sqInt> {
        let mut slots = self.lock();

        if let Some((index, slot)) = slots
            .iter_mut()
            .enumerate()
            .find(|(_, s)| s.value.is_none() && s.generation < MAX_GENERATION)
        {
            slot.generation += 1;
            slot.value = Some(value);
            return encode(index, slot.generation);
        }

        let index = slots.len();
        slots.push(Slot {
            generation: 1,
            value: Some(value),
        });
        encode(index, 1)
    }

    /// Runs `f` on the resource `handle` names.
    ///
    /// Fails with [`PrimErr::NotFound`] if the handle is malformed, already
    /// removed, or from an earlier occupant of the slot.
    pub fn with<R>(&self, handle: sqInt, f: impl FnOnce(&T) -> R) -> PrimResult<R> {
        let slots = self.lock();
        Ok(f(resolve(&slots, handle)?))
    }

    /// Runs `f` on the resource `handle` names, mutably.
    pub fn with_mut<R>(&self, handle: sqInt, f: impl FnOnce(&mut T) -> R) -> PrimResult<R> {
        let (index, generation) = decode(handle)?;
        let mut slots = self.lock();
        let slot = slots.get_mut(index).ok_or(PrimErr::NotFound)?;
        if slot.generation != generation {
            return Err(PrimErr::NotFound);
        }
        let value = slot.value.as_mut().ok_or(PrimErr::NotFound)?;
        Ok(f(value))
    }

    /// Takes the resource out of the registry, invalidating `handle`.
    ///
    /// The caller owns the value and is responsible for releasing it. Removing
    /// twice fails the second time rather than double-freeing.
    pub fn remove(&self, handle: sqInt) -> PrimResult<T> {
        let (index, generation) = decode(handle)?;
        let mut slots = self.lock();
        let slot = slots.get_mut(index).ok_or(PrimErr::NotFound)?;
        if slot.generation != generation {
            return Err(PrimErr::NotFound);
        }
        slot.value.take().ok_or(PrimErr::NotFound)
    }

    /// Is this a handle on a live resource?
    #[must_use]
    pub fn is_live(&self, handle: sqInt) -> bool {
        let slots = self.lock();
        resolve(&slots, handle).is_ok()
    }

    /// How many resources are live.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lock().iter().filter(|s| s.value.is_some()).count()
    }

    /// Are there no live resources?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The handles of every live resource `predicate` accepts.
    ///
    /// For a library whose objects own each other -- SDL destroys a window's
    /// renderer with it, and a renderer's textures with that -- where one
    /// destroy call invalidates handles the image is still holding. Answering
    /// the handles rather than references keeps the lock from being held while
    /// the caller acts on them.
    #[must_use]
    pub fn handles_where(&self, predicate: impl Fn(&T) -> bool) -> Vec<sqInt> {
        let slots = self.lock();
        slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| {
                let value = slot.value.as_ref()?;
                predicate(value).then(|| encode(index, slot.generation).ok())?
            })
            .collect()
    }

    /// Empties the registry, answering everything that was in it.
    ///
    /// For a module shutdown hook: the caller releases each resource. Handles
    /// the image still holds are all invalidated, and stay invalidated,
    /// because every emptied slot's generation moves on before it is reused.
    #[must_use]
    pub fn drain(&self) -> Vec<T> {
        let mut slots = self.lock();
        slots.iter_mut().filter_map(|s| s.value.take()).collect()
    }
}

fn encode(index: usize, generation: u32) -> PrimResult<sqInt> {
    let index = sqInt::try_from(index).map_err(|_| PrimErr::LimitExceeded)?;
    if index > INDEX_MASK {
        return Err(PrimErr::LimitExceeded);
    }
    Ok(((generation as sqInt) << INDEX_BITS) | index)
}

fn decode(handle: sqInt) -> PrimResult<(usize, u32)> {
    if handle <= 0 {
        return Err(PrimErr::NotFound);
    }
    let index = usize::try_from(handle & INDEX_MASK).map_err(|_| PrimErr::NotFound)?;
    let generation = u32::try_from(handle >> INDEX_BITS).map_err(|_| PrimErr::NotFound)?;
    if generation == 0 {
        return Err(PrimErr::NotFound);
    }
    Ok((index, generation))
}

fn resolve<T>(slots: &[Slot<T>], handle: sqInt) -> PrimResult<&T> {
    let (index, generation) = decode(handle)?;
    let slot = slots.get(index).ok_or(PrimErr::NotFound)?;
    if slot.generation != generation {
        return Err(PrimErr::NotFound);
    }
    slot.value.as_ref().ok_or(PrimErr::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_handle_round_trips() {
        let reg: Registry<u32> = Registry::new();
        let h = reg.insert(42).unwrap();
        assert_eq!(reg.with(h, |v| *v).unwrap(), 42);
    }

    #[test]
    fn zero_is_never_a_valid_handle() {
        let reg: Registry<u32> = Registry::new();
        reg.insert(1).unwrap();
        assert_eq!(reg.with(0, |v| *v).unwrap_err(), PrimErr::NotFound);
        assert!(!reg.is_live(0));
    }

    #[test]
    fn negative_handles_are_rejected_rather_than_wrapping() {
        let reg: Registry<u32> = Registry::new();
        assert_eq!(reg.with(-1, |v| *v).unwrap_err(), PrimErr::NotFound);
    }

    #[test]
    fn removing_twice_fails_instead_of_double_freeing() {
        let reg: Registry<u32> = Registry::new();
        let h = reg.insert(7).unwrap();
        assert_eq!(reg.remove(h).unwrap(), 7);
        assert_eq!(reg.remove(h).unwrap_err(), PrimErr::NotFound);
    }

    #[test]
    fn a_stale_handle_does_not_resolve_to_the_slots_new_occupant() {
        // The hole this exists to close: SurfacePlugin's registry reuses the
        // lowest free index and a stale ID silently names the newcomer.
        let reg: Registry<u32> = Registry::new();
        let first = reg.insert(1).unwrap();
        reg.remove(first).unwrap();
        let second = reg.insert(2).unwrap();

        assert_ne!(first, second);
        assert_eq!(first & INDEX_MASK, second & INDEX_MASK); // same slot
        assert_eq!(reg.with(first, |v| *v).unwrap_err(), PrimErr::NotFound);
        assert_eq!(reg.with(second, |v| *v).unwrap(), 2);
    }

    #[test]
    fn handles_stay_small_integers() {
        use crate::proxy::MAX_SMALL_INTEGER;
        let reg: Registry<u32> = Registry::new();
        for i in 0..64 {
            let h = reg.insert(i).unwrap();
            assert!(h > 0 && h <= MAX_SMALL_INTEGER, "handle {h} not immediate");
        }
    }

    #[test]
    fn mutation_is_visible_to_later_lookups() {
        let reg: Registry<u32> = Registry::new();
        let h = reg.insert(1).unwrap();
        reg.with_mut(h, |v| *v = 9).unwrap();
        assert_eq!(reg.with(h, |v| *v).unwrap(), 9);
    }

    #[test]
    fn draining_empties_the_registry_and_invalidates_handles() {
        let reg: Registry<u32> = Registry::new();
        let a = reg.insert(1).unwrap();
        let b = reg.insert(2).unwrap();
        let mut drained = reg.drain();
        drained.sort_unstable();
        assert_eq!(drained, vec![1, 2]);
        assert!(reg.is_empty());
        assert!(!reg.is_live(a));
        assert!(!reg.is_live(b));
    }

    #[test]
    fn handles_where_finds_live_matches_only() {
        let reg: Registry<u32> = Registry::new();
        let a = reg.insert(1).unwrap();
        let b = reg.insert(2).unwrap();
        let c = reg.insert(1).unwrap();
        reg.remove(c).unwrap();

        let mut found = reg.handles_where(|v| *v == 1);
        found.sort_unstable();
        assert_eq!(found, vec![a], "the removed slot must not come back");
        assert_eq!(reg.handles_where(|v| *v == 2), vec![b]);
        assert!(reg.handles_where(|v| *v == 99).is_empty());
    }

    #[test]
    fn len_counts_only_live_resources() {
        let reg: Registry<u32> = Registry::new();
        let a = reg.insert(1).unwrap();
        reg.insert(2).unwrap();
        assert_eq!(reg.len(), 2);
        reg.remove(a).unwrap();
        assert_eq!(reg.len(), 1);
    }
}
