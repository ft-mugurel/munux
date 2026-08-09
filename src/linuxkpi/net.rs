//! linuxkpi NIC register / RX / ICMP ping.

use crate::net;

pub type XmitFn = extern "C" fn(*const u8, u32) -> i32;
pub type PollFn = extern "C" fn();

pub extern "C" fn munux_register_nic(
    _name: *const u8,
    mac: *const u8,
    xmit: Option<XmitFn>,
    poll: Option<PollFn>,
) -> i32 {
    if mac.is_null() {
        return -22;
    }
    let (Some(x), Some(p)) = (xmit, poll) else {
        return -22;
    };
    let mut m = [0u8; 6];
    unsafe {
        core::ptr::copy_nonoverlapping(mac, m.as_mut_ptr(), 6);
    }
    net::register_nic(m, x, p)
}

pub extern "C" fn munux_unregister_nic() {
    net::unregister_nic();
}

pub extern "C" fn munux_netif_rx(frame: *const u8, len: u32) {
    if frame.is_null() || len < 14 {
        return;
    }
    let n = len.min(1514) as usize;
    let s = unsafe { core::slice::from_raw_parts(frame, n) };
    net::netif_rx(s);
}

/// `dst` is host-order IPv4 (0 → 10.0.2.2). timeout in jiffies.
pub extern "C" fn munux_icmp_ping(dst: u32, timeout: u64) -> i32 {
    let t = if timeout == 0 { 300 } else { timeout };
    net::icmp_ping(dst, t)
}
