// Driver globalThis-capture: deleting the `globalThis` binding (as
// test262's global/property-descriptor test does via isConfigurable's
// `delete obj[name]`) must not break markers — the driver captured the
// object before the program ran. Member-form delete: strict-safe,
// same effect as deleting the binding.
delete globalThis.globalThis;
var globalthis_delete_ok = 1;
