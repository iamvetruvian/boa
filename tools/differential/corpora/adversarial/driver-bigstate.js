// Driver render cap: a megabyte global must render truncated (512 chars
// + length suffix), not bloat the state marker past the supervisor's
// bounded capture and SIGPIPE both children.
var huge = new Array(1000000).join("xyzzy");
var bigstate_ok = 1;
