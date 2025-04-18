use crate::{phy::DeviceCapabilities, wire::*};

#[allow(clippy::large_enum_variant)]
#[derive(Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg(feature = "medium-ethernet")]
pub(crate) enum EthernetPacket<'a> {
    #[cfg(feature = "proto-ipv4")]
    Arp(ArpRepr),
    Ip(IpPacket<'a>),
}

#[derive(Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub(crate) enum IpPacket<'p> {
    #[cfg(feature = "proto-ipv4")]
    Ipv4(Ipv4Packet<'p>),
    #[cfg(feature = "proto-ipv6")]
    Ipv6(Ipv6Packet<'p>),
}

impl<'p> IpPacket<'p> {
    pub(crate) fn new(ip_repr: IpRepr, payload: IpPayload<'p>) -> Self {
        match ip_repr {
            #[cfg(feature = "proto-ipv4")]
            IpRepr::Ipv4(header) => Self::new_ipv4(header, payload),
            #[cfg(feature = "proto-ipv6")]
            IpRepr::Ipv6(header) => Self::new_ipv6(header, payload),
        }
    }

    #[cfg(feature = "proto-ipv4")]
    pub(crate) fn new_ipv4(ip_repr: Ipv4Repr, payload: IpPayload<'p>) -> Self {
        Self::Ipv4(Ipv4Packet {
            header: ip_repr,
            payload,
        })
    }

    #[cfg(feature = "proto-ipv6")]
    pub(crate) fn new_ipv6(ip_repr: Ipv6Repr, payload: IpPayload<'p>) -> Self {
        Self::Ipv6(Ipv6Packet {
            header: ip_repr,
            #[cfg(feature = "proto-ipv6-hbh")]
            hop_by_hop: None,
            #[cfg(feature = "proto-ipv6-fragmentation")]
            fragment: None,
            #[cfg(feature = "proto-ipv6-routing")]
            routing: None,
            payload,
        })
    }

    pub(crate) fn ip_repr(&self) -> IpRepr {
        match self {
            #[cfg(feature = "proto-ipv4")]
            IpPacket::Ipv4(p) => IpRepr::Ipv4(p.header),
            #[cfg(feature = "proto-ipv6")]
            IpPacket::Ipv6(p) => IpRepr::Ipv6(p.header),
        }
    }

    pub(crate) fn payload(&self) -> &IpPayload<'p> {
        match self {
            #[cfg(feature = "proto-ipv4")]
            IpPacket::Ipv4(p) => &p.payload,
            #[cfg(feature = "proto-ipv6")]
            IpPacket::Ipv6(p) => &p.payload,
        }
    }

    pub(crate) fn emit_payload(
        &self,
        _ip_repr: &IpRepr,
        payload: &mut [u8],
        caps: &DeviceCapabilities,
    ) {
        match self.payload() {
            #[cfg(feature = "proto-ipv4")]
            IpPayload::Icmpv4(icmpv4_repr) => {
                icmpv4_repr.emit(&mut Icmpv4Packet::new_unchecked(payload), &caps.checksum)
            }
            #[cfg(feature = "proto-igmp")]
            IpPayload::Igmp(igmp_repr) => igmp_repr.emit(&mut IgmpPacket::new_unchecked(payload)),
            #[cfg(feature = "proto-ipv6")]
            IpPayload::Icmpv6(icmpv6_repr) => icmpv6_repr.emit(
                &_ip_repr.src_addr(),
                &_ip_repr.dst_addr(),
                &mut Icmpv6Packet::new_unchecked(payload),
                &caps.checksum,
            ),
            #[cfg(feature = "socket-raw")]
            IpPayload::Raw(raw_packet) => payload.copy_from_slice(raw_packet),
            #[cfg(any(feature = "socket-udp", feature = "socket-dns"))]
            IpPayload::Udp(udp_repr, inner_payload) => udp_repr.emit(
                &mut UdpPacket::new_unchecked(payload),
                &_ip_repr.src_addr(),
                &_ip_repr.dst_addr(),
                inner_payload.len(),
                |buf| buf.copy_from_slice(inner_payload),
                &caps.checksum,
            ),
            #[cfg(feature = "socket-tcp")]
            IpPayload::Tcp(mut tcp_repr) => {
                // This is a terrible hack to make TCP performance more acceptable on systems
                // where the TCP buffers are significantly larger than network buffers,
                // e.g. a 64 kB TCP receive buffer (and so, when empty, a 64k window)
                // together with four 1500 B Ethernet receive buffers. If left untreated,
                // this would result in our peer pushing our window and sever packet loss.
                //
                // I'm really not happy about this "solution" but I don't know what else to do.
                if let Some(max_burst_size) = caps.max_burst_size {
                    let mut max_segment_size = caps.max_transmission_unit;
                    max_segment_size -= _ip_repr.header_len();
                    max_segment_size -= tcp_repr.header_len();

                    let max_window_size = max_burst_size * max_segment_size;
                    if tcp_repr.window_len as usize > max_window_size {
                        tcp_repr.window_len = max_window_size as u16;
                    }
                }

                tcp_repr.emit(
                    &mut TcpPacket::new_unchecked(payload),
                    &_ip_repr.src_addr(),
                    &_ip_repr.dst_addr(),
                    &caps.checksum,
                );
            }
            #[cfg(feature = "socket-dhcpv4")]
            IpPayload::Dhcpv4(udp_repr, dhcp_repr) => udp_repr.emit(
                &mut UdpPacket::new_unchecked(payload),
                &_ip_repr.src_addr(),
                &_ip_repr.dst_addr(),
                dhcp_repr.buffer_len(),
                |buf| dhcp_repr.emit(&mut DhcpPacket::new_unchecked(buf)).unwrap(),
                &caps.checksum,
            ),
        }
    }
}

#[derive(Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg(feature = "proto-ipv4")]
pub(crate) struct Ipv4Packet<'p> {
    header: Ipv4Repr,
    payload: IpPayload<'p>,
}

#[derive(Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg(feature = "proto-ipv6")]
pub(crate) struct Ipv6Packet<'p> {
    header: Ipv6Repr,
    #[cfg(feature = "proto-ipv6-hbh")]
    hop_by_hop: Option<Ipv6HopByHopRepr<'p>>,
    #[cfg(feature = "proto-ipv6-fragmentation")]
    fragment: Option<Ipv6FragmentRepr>,
    #[cfg(feature = "proto-ipv6-routing")]
    routing: Option<Ipv6RoutingRepr<'p>>,
    payload: IpPayload<'p>,
}

#[derive(Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub(crate) enum IpPayload<'p> {
    #[cfg(feature = "proto-ipv4")]
    Icmpv4(Icmpv4Repr<'p>),
    #[cfg(feature = "proto-igmp")]
    Igmp(IgmpRepr),
    #[cfg(feature = "proto-ipv6")]
    Icmpv6(Icmpv6Repr<'p>),
    #[cfg(feature = "socket-raw")]
    Raw(&'p [u8]),
    #[cfg(any(feature = "socket-udp", feature = "socket-dns"))]
    Udp(UdpRepr, &'p [u8]),
    #[cfg(feature = "socket-tcp")]
    Tcp(TcpRepr<'p>),
    #[cfg(feature = "socket-dhcpv4")]
    Dhcpv4(UdpRepr, DhcpRepr<'p>),
}

#[cfg(any(feature = "proto-ipv4", feature = "proto-ipv6"))]
pub(crate) fn icmp_reply_payload_len(len: usize, mtu: usize, header_len: usize) -> usize {
    // Send back as much of the original payload as will fit within
    // the minimum MTU required by IPv4. See RFC 1812 § 4.3.2.3 for
    // more details.
    //
    // Since the entire network layer packet must fit within the minimum
    // MTU supported, the payload must not exceed the following:
    //
    // <min mtu> - IP Header Size * 2 - ICMPv4 DstUnreachable hdr size
    len.min(mtu - header_len * 2 - 8)
}

#[cfg(feature = "proto-igmp")]
pub(crate) enum IgmpReportState {
    Inactive,
    ToGeneralQuery {
        version: IgmpVersion,
        timeout: crate::time::Instant,
        interval: crate::time::Duration,
        next_index: usize,
    },
    ToSpecificQuery {
        version: IgmpVersion,
        timeout: crate::time::Instant,
        group: Ipv4Address,
    },
}
