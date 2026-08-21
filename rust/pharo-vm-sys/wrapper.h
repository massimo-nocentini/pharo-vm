/* Bindgen entry point for the Pharo VM's C headers.
 *
 * Everything Rust needs to see from C is reached from here. Each migration
 * wave adds the header it needs, and adds matching entries to the allowlists
 * in build.rs -- the allowlists are what keep the generated bindings small and
 * reviewable, so please do not drop them in favour of binding the world.
 *
 * Note that pharo.h transitively includes the *generated* interp.h, so this
 * file can only be parsed once VMMaker has produced the C sources (or a
 * pre-generated tree is present). build.rs enforces and explains that.
 */

/* Wave 0 */
#include "pharovm/errorCode.h"

/* Interpreter proxy (plugin ABI) */
#include "pharovm/common/virtualMachine.h"

/* Wave 1 */
#include "pharovm/parameters/parameterVector.h"

/* Wave 2 -- image file access.
 *
 * imageAccess.h names sqInt and EXPORT without including anything that
 * defines them; it is only ever reached through sq.h, so reach it the same
 * way rather than including it bare. */
#include "pharovm/common/sq.h"
#include "pharovm/imageAccess.h"

/* Wave 5 -- semaphores. The Semaphore vtable is shared with the FFI worker and
 * callback code in src/ffi/, which is still C. */
#include "pharovm/semaphores/pSemaphore.h"

/* Wave 9 -- command-line parameters. */
#include "pharovm/parameters/parameters.h"

/* Wave 10 -- the VM's startup path. */
#include "pharovm/pharoClient.h"
#include "pharovm/fileDialog.h"
