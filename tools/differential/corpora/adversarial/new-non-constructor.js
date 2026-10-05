// P2 seam: `new` on a non-constructor throws TypeError (kind agrees).
try { new (42)(); } catch (e) { e.constructor.name; }
