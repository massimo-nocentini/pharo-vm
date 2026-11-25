// host-events.js - --pre-js of both Pharo VMs on wasm: events of the host signal the image
//
// Module.pharoHost, for the services of the JavaScript side that work while
// the VM sleeps or runs other slices (web-fetch.js, linked after this file):
//
//   signal(index)    signal the external semaphore of the image and wake the
//                    VM: the wakeup of a SLEEPING VM is cut short.  Answers
//                    false for an index that the table of the VM does not
//                    hold, and after shutdown()
//   add(service)     a service: {networkOff(), shutdown()}
//   network          whether the image may use the network
//   setNetwork(on)   change that; when off, networkOff() of every service
//   shutdown()       the VM is dead: no signal any more, shutdown() of every
//                    service, once
//
// The rule of every service: it keeps the index of the semaphore in a record
// of its own, forgets the record synchronously, in the primitive with which
// the image closes it, and signals only for a record that it still has.  The
// image unregisters the semaphore after that, so a late event never reaches
// the index once another semaphore has it.
//
// The hosts (vm-driver.js, node-pump.js) set Module.hostWake() and
// Module.hostNetwork (false: off) later than this runs, so both are read at
// each use.  A plain script: in the factory of the web VM, a module.exports
// would replace the one of the factory.

Module['pharoHost'] = (() => {
  const services = [];
  let gone = false;
  const report = (what, e) => { (Module['printErr'] || console.error)('pharoHost.' + what + ': ' + e); };
  const each = name => {
    for (const service of services) {
      try { service[name](); } catch (e) { report(name, e); }
    }
  };
  return {
    signal(index) {
      if (gone) return false;
      try {
        if (!Module['_vm_signal_semaphore'](index)) return false;
        if (Module['hostWake']) Module['hostWake']();
      } catch (e) {
        report('signal', e);
        return false;
      }
      return true;
    },
    add(service) { services.push(service); },
    get network() { return Module['hostNetwork'] !== false; },
    setNetwork(on) {
      Module['hostNetwork'] = !!on;
      if (!on) each('networkOff');
    },
    shutdown() {
      if (gone) return;
      gone = true;
      each('shutdown');
    },
  };
})();
