//! Reserved for the future "no-launcher install" path.
//!
//! Originally this module was going to forward dxgi.dll exports so the
//! loader could be dropped next to SCUM.exe and auto-load. The dxgi-proxy
//! pattern has a name-collision footgun (a dll named dxgi.dll can't .def-
//! forward to dxgi.dll) and we have a launcher anyway, so this is empty
//! for now. Bring it back if/when we want users to skip the launcher.

#![allow(dead_code)]
