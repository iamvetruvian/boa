// Driver lone-surrogate transport: observed strings with unpaired
// surrogates must survive the marker codec (strict JSON parsers reject
// bare halves; the capture re-escapes them losslessly for comparison).
// Both sides run the identical codec. Explicit escapes keep this file
// itself valid UTF-8.
var lone_high = "ab\ud800cd";
var lone_low = "ab\udc00cd";
var well_formed_pair = "ab\ud83d\ude00cd";
var surrogate_transport_ok = 1;
