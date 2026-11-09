use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{Read, Take};
use std::path::Path;

const MAX_INTERFACES: usize = 16;
const MAX_INTERFACE_NAME_BYTES: usize = 15;
const MAX_PROC_NET_DEV_BYTES: u64 = 16 * 1024;
const MAX_PROC_TCP_BYTES: u64 = 4 * 1024 * 1024;
const MAX_TCP_SOCKETS: u32 = 200_000;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InterfaceCounters {
    pub(crate) name: String,
    pub(crate) loopback: bool,
    pub(crate) rx_bytes: u64,
    pub(crate) rx_packets: u64,
    pub(crate) rx_errors: u64,
    pub(crate) rx_dropped: u64,
    pub(crate) tx_bytes: u64,
    pub(crate) tx_packets: u64,
    pub(crate) tx_errors: u64,
    pub(crate) tx_dropped: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TcpStateCounts {
    pub(crate) established: u32,
    pub(crate) syn_sent: u32,
    pub(crate) syn_received: u32,
    pub(crate) fin_wait_1: u32,
    pub(crate) fin_wait_2: u32,
    pub(crate) time_wait: u32,
    pub(crate) close: u32,
    pub(crate) close_wait: u32,
    pub(crate) last_ack: u32,
    pub(crate) listen: u32,
    pub(crate) closing: u32,
    pub(crate) other: u32,
}

impl TcpStateCounts {
    pub(crate) fn total(&self) -> u32 {
        [
            self.established,
            self.syn_sent,
            self.syn_received,
            self.fin_wait_1,
            self.fin_wait_2,
            self.time_wait,
            self.close,
            self.close_wait,
            self.last_ack,
            self.listen,
            self.closing,
            self.other,
        ]
        .into_iter()
        .fold(0_u32, u32::saturating_add)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NetworkSnapshot {
    pub(crate) interfaces: Vec<InterfaceCounters>,
    pub(crate) tcp_states: TcpStateCounts,
}

pub(crate) fn read_network_snapshot() -> Option<NetworkSnapshot> {
    let interfaces = parse_net_dev(&read_bounded(
        Path::new("/proc/net/dev"),
        MAX_PROC_NET_DEV_BYTES,
    )?)?;
    let tcp = read_bounded(Path::new("/proc/net/tcp"), MAX_PROC_TCP_BYTES)?;
    let tcp6 = read_bounded(Path::new("/proc/net/tcp6"), MAX_PROC_TCP_BYTES)?;
    let snapshot = NetworkSnapshot {
        interfaces,
        tcp_states: parse_tcp_states(&tcp, &tcp6)?,
    };
    valid_network_snapshot(&snapshot).then_some(snapshot)
}

pub(crate) fn parse_net_dev(contents: &str) -> Option<Vec<InterfaceCounters>> {
    if contents.len() > MAX_PROC_NET_DEV_BYTES as usize {
        return None;
    }
    let mut interfaces = Vec::new();
    let mut names = HashSet::new();
    for line in contents.lines().skip(2) {
        if line.trim().is_empty() {
            continue;
        }
        let (name, counters) = line.split_once(':')?;
        let name = name.trim();
        if !valid_interface_name(name) || !names.insert(name.to_owned()) {
            return None;
        }
        let values = counters
            .split_whitespace()
            .map(str::parse::<u64>)
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        if values.len() != 16 || interfaces.len() >= MAX_INTERFACES {
            return None;
        }
        interfaces.push(InterfaceCounters {
            name: name.to_owned(),
            loopback: name == "lo",
            rx_bytes: values[0],
            rx_packets: values[1],
            rx_errors: values[2],
            rx_dropped: values[3],
            tx_bytes: values[8],
            tx_packets: values[9],
            tx_errors: values[10],
            tx_dropped: values[11],
        });
    }
    Some(interfaces)
}

pub(crate) fn parse_tcp_states(tcp: &str, tcp6: &str) -> Option<TcpStateCounts> {
    let mut states = TcpStateCounts::default();
    let mut total = 0_u32;
    for contents in [tcp, tcp6] {
        for line in contents.lines().skip(1) {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let state = line.split_whitespace().nth(3)?;
            let state = u8::from_str_radix(state, 16).ok()?;
            total = total.checked_add(1)?;
            if total > MAX_TCP_SOCKETS {
                return None;
            }
            let counter = match state {
                0x01 => &mut states.established,
                0x02 => &mut states.syn_sent,
                0x03 => &mut states.syn_received,
                0x04 => &mut states.fin_wait_1,
                0x05 => &mut states.fin_wait_2,
                0x06 => &mut states.time_wait,
                0x07 => &mut states.close,
                0x08 => &mut states.close_wait,
                0x09 => &mut states.last_ack,
                0x0a => &mut states.listen,
                0x0b => &mut states.closing,
                _ => &mut states.other,
            };
            *counter = counter.checked_add(1)?;
        }
    }
    Some(states)
}

pub(crate) fn valid_network_snapshot(snapshot: &NetworkSnapshot) -> bool {
    snapshot.interfaces.len() <= MAX_INTERFACES
        && snapshot.tcp_states.total() <= MAX_TCP_SOCKETS
        && snapshot.interfaces.iter().all(|interface| {
            valid_interface_name(&interface.name) && interface.loopback == (interface.name == "lo")
        })
        && snapshot
            .interfaces
            .iter()
            .map(|interface| interface.name.as_str())
            .collect::<HashSet<_>>()
            .len()
            == snapshot.interfaces.len()
        && metric_sum_fits(&snapshot.interfaces, |item| item.rx_bytes)
        && metric_sum_fits(&snapshot.interfaces, |item| item.tx_bytes)
        && metric_sum_fits(&snapshot.interfaces, |item| item.rx_errors)
        && metric_sum_fits(&snapshot.interfaces, |item| item.tx_errors)
        && metric_sum_fits(&snapshot.interfaces, |item| item.rx_dropped)
        && metric_sum_fits(&snapshot.interfaces, |item| item.tx_dropped)
}

fn metric_sum_fits(
    interfaces: &[InterfaceCounters],
    metric: impl Fn(&InterfaceCounters) -> u64,
) -> bool {
    interfaces
        .iter()
        .filter(|item| !item.loopback)
        .try_fold(0_u64, |total, item| total.checked_add(metric(item)))
        .is_some_and(|total| total <= i64::MAX as u64)
}

fn valid_interface_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_INTERFACE_NAME_BYTES
        && name
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b':')
}

fn read_bounded(path: &Path, maximum: u64) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut reader: Take<std::fs::File> = file.take(maximum.saturating_add(1));
    let mut contents = String::new();
    reader.read_to_string(&mut contents).ok()?;
    (contents.len() as u64 <= maximum).then_some(contents)
}

#[cfg(test)]
mod tests {
    use super::{parse_net_dev, parse_tcp_states, valid_network_snapshot, NetworkSnapshot};

    #[test]
    fn parses_interface_counters_and_preserves_loopback_separately() {
        let input = " Inter-| Receive | Transmit\n face |bytes packets errs drop fifo frame compressed multicast|bytes packets errs drop fifo colls carrier compressed\n lo: 100 2 0 0 0 0 0 0 100 2 0 0 0 0 0 0\n eth0: 900 9 1 2 0 0 0 0 800 8 3 4 0 0 0 0\n";
        let interfaces = parse_net_dev(input).unwrap();

        assert_eq!(interfaces.len(), 2);
        assert_eq!(interfaces[0].name, "lo");
        assert!(interfaces[0].loopback);
        assert_eq!(interfaces[0].rx_bytes, 100);
        assert_eq!(interfaces[0].tx_packets, 2);
        assert_eq!(interfaces[1].name, "eth0");
        assert!(!interfaces[1].loopback);
        assert_eq!(interfaces[1].rx_errors, 1);
        assert_eq!(interfaces[1].rx_dropped, 2);
        assert_eq!(interfaces[1].tx_errors, 3);
        assert_eq!(interfaces[1].tx_dropped, 4);
    }

    #[test]
    fn rejects_malformed_duplicate_and_unbounded_interface_rows() {
        let headers = " Inter-| Receive | Transmit\n face |bytes packets errs drop fifo frame compressed multicast|bytes packets errs drop fifo colls carrier compressed\n";
        assert!(parse_net_dev(&format!("{headers}eth0: 1 2\n")).is_none());
        assert!(parse_net_dev(&format!("{headers}eth0: 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16\neth0: 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16\n")).is_none());
        let many = (0..17)
            .map(|index| format!("eth{index}: 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16\n"))
            .collect::<String>();
        assert!(parse_net_dev(&format!("{headers}{many}")).is_none());
    }

    #[test]
    fn counts_tcp_states_without_retaining_endpoints() {
        let header = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n";
        let ipv4 = format!(
            "{header} 0: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000 1000 0 123\n 1: 0100007F:1F91 0100007F:1F92 01 00000000:00000000 00:00000000 00000000 1000 0 124\n"
        );
        let ipv6 = format!("{header} 0: 00000000000000000000000001000000:1F90 00000000000000000000000000000000:0000 06 00000000:00000000 00:00000000 00000000 1000 0 125\n");
        let states = parse_tcp_states(&ipv4, &ipv6).unwrap();

        assert_eq!(states.listen, 1);
        assert_eq!(states.established, 1);
        assert_eq!(states.time_wait, 1);
        assert_eq!(states.total(), 3);
    }

    #[test]
    fn validates_bounds_before_accepting_serialized_network_data() {
        let snapshot = NetworkSnapshot {
            interfaces: vec![],
            tcp_states: Default::default(),
        };
        assert!(valid_network_snapshot(&snapshot));
        let too_many = NetworkSnapshot {
            interfaces: (0..17)
                .map(|index| super::InterfaceCounters {
                    name: format!("eth{index}"),
                    loopback: false,
                    rx_bytes: 0,
                    rx_packets: 0,
                    rx_errors: 0,
                    rx_dropped: 0,
                    tx_bytes: 0,
                    tx_packets: 0,
                    tx_errors: 0,
                    tx_dropped: 0,
                })
                .collect(),
            tcp_states: Default::default(),
        };
        assert!(!valid_network_snapshot(&too_many));
    }
}
