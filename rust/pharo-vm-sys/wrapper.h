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
