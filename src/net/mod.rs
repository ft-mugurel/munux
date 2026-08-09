//! Minimal Ethernet + ARP + IPv4 ICMP (virtio-net smoke / P12 start).

use crate::console;
use crate::interrupts::timer;
use crate::interrupts::utils::enable_interrupts;

const ETH_ALEN: usize = 6;
const ETH_HLEN: usize = 14;
const ETH_ARP: u16 = 0x0806;
const ETH_IP: u16 = 0x0800;
const ARP_REQUEST: u16 = 1;
const ARP_REPLY: u16 = 2;
const IPPROTO_ICMP: u8 = 1;
const ICMP_ECHO: u8 = 8;
const ICMP_ECHOREPLY: u8 = 0;

pub const GUEST_IP: u32 = 0x0A00_020F; // 10.0.2.15
pub const GW_IP: u32 = 0x0A00_0202; // 10.0.2.2

type XmitFn = extern "C" fn(*const u8, u32) -> i32;
type PollFn = extern "C" fn();

struct Nic {
    used: bool,
    mac: [u8; ETH_ALEN],
    xmit: Option<XmitFn>,
    poll: Option<PollFn>,
}

static mut NIC: Nic = Nic {
    used: false,
    mac: [0; ETH_ALEN],
    xmit: None,
    poll: None,
};

struct Ping {
    active: bool,
    dst: u32,
    id: u16,
    seq: u16,
    arp_ok: bool,
    reply: bool,
    gw_mac: [u8; ETH_ALEN],
}

static mut PING: Ping = Ping {
    active: false,
    dst: 0,
    id: 0x4242,
    seq: 1,
    arp_ok: false,
    reply: false,
    gw_mac: [0; ETH_ALEN],
};

fn nic_mut() -> &'static mut Nic {
    unsafe { &mut *core::ptr::addr_of_mut!(NIC) }
}

fn ping_mut() -> &'static mut Ping {
    unsafe { &mut *core::ptr::addr_of_mut!(PING) }
}

fn be16(v: u16) -> [u8; 2] {
    v.to_be_bytes()
}

fn be32(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}

fn rd16(b: &[u8]) -> u16 {
    u16::from_be_bytes([b[0], b[1]])
}

fn rd32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

fn checksum(data: &[u8]) -> u16 {
    let mut sum = 0u32;
    let mut i = 0;
    while i + 1 < data.len() {
        sum += u16::from_be_bytes([data[i], data[i + 1]]) as u32;
        i += 2;
    }
    if i < data.len() {
        sum += (data[i] as u32) << 8;
    }
    while (sum >> 16) != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

fn xmit(frame: &[u8]) -> i32 {
    let n = nic_mut();
    let Some(f) = n.xmit else {
        return -19; // ENODEV
    };
    f(frame.as_ptr(), frame.len() as u32)
}

fn poll_hw() {
    if let Some(p) = nic_mut().poll {
        p();
    }
}

fn send_arp_request(dst_ip: u32) {
    let mac = nic_mut().mac;
    let mut f = [0u8; 42];
    f[0..6].copy_from_slice(&[0xff; 6]);
    f[6..12].copy_from_slice(&mac);
    f[12..14].copy_from_slice(&be16(ETH_ARP));
    f[14..16].copy_from_slice(&be16(1));
    f[16..18].copy_from_slice(&be16(ETH_IP));
    f[18] = 6;
    f[19] = 4;
    f[20..22].copy_from_slice(&be16(ARP_REQUEST));
    f[22..28].copy_from_slice(&mac);
    f[28..32].copy_from_slice(&be32(GUEST_IP));
    f[32..38].copy_from_slice(&[0; 6]);
    f[38..42].copy_from_slice(&be32(dst_ip));
    let _ = xmit(&f);
}

fn send_arp_reply(tha: &[u8; 6], tpa: u32) {
    let mac = nic_mut().mac;
    let mut f = [0u8; 42];
    f[0..6].copy_from_slice(tha);
    f[6..12].copy_from_slice(&mac);
    f[12..14].copy_from_slice(&be16(ETH_ARP));
    f[14..16].copy_from_slice(&be16(1));
    f[16..18].copy_from_slice(&be16(ETH_IP));
    f[18] = 6;
    f[19] = 4;
    f[20..22].copy_from_slice(&be16(ARP_REPLY));
    f[22..28].copy_from_slice(&mac);
    f[28..32].copy_from_slice(&be32(GUEST_IP));
    f[32..38].copy_from_slice(tha);
    f[38..42].copy_from_slice(&be32(tpa));
    let _ = xmit(&f);
}

fn send_icmp_echo(dst_ip: u32, dmac: &[u8; 6], id: u16, seq: u16) {
    let mac = nic_mut().mac;
    let mut f = [0u8; 14 + 20 + 8 + 8];
    f[0..6].copy_from_slice(dmac);
    f[6..12].copy_from_slice(&mac);
    f[12..14].copy_from_slice(&be16(ETH_IP));
    // IPv4
    f[14] = 0x45;
    f[15] = 0;
    let iplen = 20 + 8 + 8;
    f[16..18].copy_from_slice(&be16(iplen));
    f[18..20].copy_from_slice(&be16(0x4242));
    f[20..22].copy_from_slice(&be16(0));
    f[22] = 64;
    f[23] = IPPROTO_ICMP;
    f[24..26].copy_from_slice(&[0, 0]);
    f[26..30].copy_from_slice(&be32(GUEST_IP));
    f[30..34].copy_from_slice(&be32(dst_ip));
    let csum = checksum(&f[14..34]);
    f[24..26].copy_from_slice(&csum.to_be_bytes());
    // ICMP
    f[34] = ICMP_ECHO;
    f[35] = 0;
    f[36..38].copy_from_slice(&[0, 0]);
    f[38..40].copy_from_slice(&be16(id));
    f[40..42].copy_from_slice(&be16(seq));
    f[42..50].copy_from_slice(b"munuxnet");
    let ics = checksum(&f[34..50]);
    f[36..38].copy_from_slice(&ics.to_be_bytes());
    let _ = xmit(&f[..50]);
}

fn handle_arp(frame: &[u8]) {
    if frame.len() < 42 {
        return;
    }
    let op = rd16(&frame[20..22]);
    let spa = rd32(&frame[28..32]);
    let tpa = rd32(&frame[38..42]);
    let mut sha = [0u8; 6];
    sha.copy_from_slice(&frame[22..28]);
    if op == ARP_REQUEST && tpa == GUEST_IP {
        send_arp_reply(&sha, spa);
    }
    if op == ARP_REPLY && tpa == GUEST_IP {
        let p = ping_mut();
        if p.active && spa == p.dst {
            p.gw_mac = sha;
            p.arp_ok = true;
        }
    }
}

fn handle_ip(frame: &[u8]) {
    if frame.len() < 14 + 20 {
        return;
    }
    let ihl = ((frame[14] & 0x0f) as usize) * 4;
    if frame[14] >> 4 != 4 || ihl < 20 {
        return;
    }
    if rd32(&frame[30..34]) != GUEST_IP {
        return;
    }
    if frame[23] != IPPROTO_ICMP {
        return;
    }
    let off = 14 + ihl;
    if frame.len() < off + 8 {
        return;
    }
    let typ = frame[off];
    let id = rd16(&frame[off + 4..off + 6]);
    let seq = rd16(&frame[off + 6..off + 8]);
    if typ == ICMP_ECHOREPLY {
        let p = ping_mut();
        if p.active && id == p.id && seq == p.seq {
            p.reply = true;
        }
    }
}

/// Incoming Ethernet frame from the NIC driver (no virtio_net_hdr).
pub fn netif_rx(frame: &[u8]) {
    if frame.len() < ETH_HLEN {
        return;
    }
    let et = rd16(&frame[12..14]);
    if et == ETH_ARP {
        handle_arp(frame);
    } else if et == ETH_IP {
        handle_ip(frame);
    }
}

pub fn register_nic(mac: [u8; 6], xmit: XmitFn, poll: PollFn) -> i32 {
    let n = nic_mut();
    if n.used {
        return -16; // EBUSY
    }
    n.used = true;
    n.mac = mac;
    n.xmit = Some(xmit);
    n.poll = Some(poll);
    console::println("net: nic registered");
    0
}

pub fn unregister_nic() {
    let n = nic_mut();
    *n = Nic {
        used: false,
        mac: [0; ETH_ALEN],
        xmit: None,
        poll: None,
    };
    let p = ping_mut();
    p.active = false;
}

/// Blocking ICMP echo to `dst` (host-order IPv4). 0 = ok.
pub fn icmp_ping(dst: u32, timeout_ticks: u64) -> i32 {
    if !nic_mut().used {
        return -19;
    }
    let dst = if dst == 0 { GW_IP } else { dst };
    {
        let p = ping_mut();
        p.active = true;
        p.dst = dst;
        p.arp_ok = false;
        p.reply = false;
        p.seq = p.seq.wrapping_add(1);
        p.gw_mac = [0; 6];
    }
    send_arp_request(dst);
    let start = timer::ticks();
    let mut last_arp = start;
    loop {
        poll_hw();
        enable_interrupts();
        if ping_mut().arp_ok {
            break;
        }
        let now = timer::ticks();
        if now.saturating_sub(start) >= timeout_ticks {
            ping_mut().active = false;
            console::println("net: arp timeout");
            return -110; // ETIMEDOUT
        }
        if now.saturating_sub(last_arp) >= 20 {
            send_arp_request(dst);
            last_arp = now;
        }
        unsafe {
            core::arch::asm!("pause", options(nomem, nostack));
        }
    }
    let (id, seq, mac) = {
        let p = ping_mut();
        (p.id, p.seq, p.gw_mac)
    };
    send_icmp_echo(dst, &mac, id, seq);
    let start2 = timer::ticks();
    loop {
        poll_hw();
        enable_interrupts();
        if ping_mut().reply {
            ping_mut().active = false;
            return 0;
        }
        if timer::ticks().saturating_sub(start2) >= timeout_ticks {
            ping_mut().active = false;
            console::println("net: icmp timeout");
            return -110;
        }
        unsafe {
            core::arch::asm!("pause", options(nomem, nostack));
        }
    }
}
