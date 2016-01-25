// Copyright 2016 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under (1) the MaidSafe.net Commercial License,
// version 1.0 or later, or (2) The General Public License (GPL), version 3, depending on which
// licence you accepted on initial access to the Software (the "Licences").
//
// By contributing code to the SAFE Network Software, or to this project generally, you agree to be
// bound by the terms of the MaidSafe Contributor Agreement, version 1.0.  This, along with the
// Licenses can be found in the root directory of this project at LICENSE, COPYING and CONTRIBUTOR.
//
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied.
//
// Please review the Licences for the specific language governing permissions and limitations
// relating to use of the SAFE Network Software.

//! # Service Discovery
//!
//! Discover other instances of your application on the local network.

#![doc(html_logo_url =
           "https://raw.githubusercontent.com/maidsafe/QA/master/Images/maidsafe_logo.png",
       html_favicon_url = "http://maidsafe.net/img/favicon.ico",
       html_root_url = "http://maidsafe.github.io/config_file_handler/")]

// For explanation of lint checks, run `rustc -W help` or see
// https://github.com/maidsafe/QA/blob/master/Documentation/Rust%20Lint%20Checks.md
#![forbid(bad_style, exceeding_bitshifts, mutable_transmutes, no_mangle_const_items,
          unknown_crate_types, warnings)]
#![deny(deprecated, drop_with_repr_extern, improper_ctypes, missing_docs,
      non_shorthand_field_patterns, overflowing_literals, plugin_as_library,
      private_no_mangle_fns, private_no_mangle_statics, stable_features, unconditional_recursion,
      unknown_lints, unsafe_code, unused, unused_allocation, unused_attributes,
      unused_comparisons, unused_features, unused_parens, while_true)]
#![warn(trivial_casts, trivial_numeric_casts, unused_extern_crates, unused_import_braces,
        unused_qualifications, unused_results)]
#![allow(box_pointers, fat_ptr_transmutes, missing_copy_implementations,
         missing_debug_implementations)]

use rand::random;
use std::error::Error;
use std::io;
use std::net::{TcpStream, UdpSocket, TcpListener};
use std::net;
use socket_addr::SocketAddr;
use std::str::FromStr;
use std::sync::{Arc, mpsc, Mutex};
use std::thread;
use std::time::Duration;
use ip::SocketAddrExt;

use net2::UdpSocketExt;

use util;
use connection::Connection;
use connection;


pub struct BroadcastAcceptor<Reply> {
    udp_socket: UdpSocket,
    stop_flag: Arc<AtomicBool>,
    _raii_joiner: RaiiThreadJoiner,
}

impl<Reply: Encodable> BroadcastAcceptor<Reply> {
    pub fn new(port: u16, reply: Reply) -> Result<BroadcastAcceptor> {
        let serialised_reply = try!(serialise(&reply));
        let udp_socket = try!(UdpSocket::bind(format!("0.0.0.0:{}", port)));
        let cloned_udp_socket = try!(udp_socket.try_clone());

        let stop_flag = Arc::new(AtomicBool::new(false));
        let cloned_stop_flag = stop_flag.clone();

        let joiner = RaiiThreadJoiner::new(thread!("ServiceDiscoveryThread", || move {
            start_accept<Payload>(cloned_udp_socket, cloned_stop_flag, serialised_reply);
        }));

        Ok(BroadcastAcceptor {
            udp_socket: udp_socket,
            stop_flag: stop_flag,
            _raii_joiner: joiner,
        })
    }

    pub fn seek_peers() -> Result<Vec<Payload>> {
        let stuff_to_send = StuffToSend;
        let mut result = Vec::with_capacity(10);

        for attempt in 0..num_attempts {
            self.udp_socket.send_to(stuff_to_send, format!("255.255.255.255:{}", self.port));
        }

        wait_for_results;
        return;
    }

    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::SeqCst);
    }

    fn start_accept(udp_socket: UdpSocket, stop_flag: Arc<AtomicBool>, reply: Vec<u8>) {
        try!(udp_socket.set_read_timeout(Some(Duration::from_secs(UDP_RX_TIMEOUT_SECS))));

        let mut buf = [0u8; 1024];

        loop {
            let (bytes_read, peer_ep) = try!(udp_socket.recv_from(&mut buf));

            if stop_flag.load(Ordering::SeqCst) {
                return;
            }

            if bytes_read > 0 {
                let mut total_bytes_written = 0;
                while total_bytes_written <= reply.len() {
                    total_bytes_written += try!(udp_socket.send_to(&reply[total_bytes_written..],
                                                                   peer_addr));
                }
            }
        }
    }
}

impl Drop for BroadcastAcceptor {
    fn stop(&mut self) {
        self.stop_flag.store(true, Ordering::SeqCst);
    }
}

















const GUID_SIZE: usize = 16;
const MAGIC_SIZE: usize = 4;
const MAGIC: [u8; MAGIC_SIZE] = ['m' as u8, 'a' as u8, 'i' as u8, 'd' as u8];
const STOP: [u8; MAGIC_SIZE] = ['s' as u8, 't' as u8, 'o' as u8, 'p' as u8];

pub type GUID = [u8; GUID_SIZE];

fn serialise_port(port: u16) -> [u8; 2] {
    [(port & 0xff) as u8, (port >> 8) as u8]
}

fn parse_port(data: &[u8]) -> u16 {
    (data[0] as u16) + ((data[1] as u16) << 8)
}

fn serialise_shutdown_value(shutdown_value: u64) -> [u8; 8] {
    [(shutdown_value & 0xff) as u8,
     (shutdown_value >> 8) as u8,
     (shutdown_value >> 16) as u8,
     (shutdown_value >> 24) as u8,
     (shutdown_value >> 32) as u8,
     (shutdown_value >> 40) as u8,
     (shutdown_value >> 48) as u8,
     (shutdown_value >> 56) as u8]
}

fn parse_shutdown_value(data: &[u8]) -> u64 {
    (data[0] as u64) + ((data[1] as u64) << 8) + ((data[2] as u64) << 16) +
    ((data[3] as u64) << 24) + ((data[4] as u64) << 32) + ((data[5] as u64) << 40) +
    ((data[6] as u64) << 48) + ((data[7] as u64) << 56)
}

pub struct BroadcastAcceptor {
    guid: GUID,
    socket: Arc<Mutex<UdpSocket>>,
    listener: Arc<Mutex<UdpSocket>>,
    listener_port: u16,
}

impl BroadcastAcceptor {
    pub fn new(port: u16) -> io::Result<BroadcastAcceptor> {
        let socket = try!(UdpSocket::bind(("0.0.0.0", port)));
        let listener = try!(UdpSocket::bind("0.0.0.0:0"));
        let mut guid = [0; GUID_SIZE];
        for item in &mut guid {
            *item = random::<u8>();
        }
        let listener_port = unwrap_result!(listener.local_addr()).port();
        Ok(BroadcastAcceptor {
            guid: guid,
            socket: Arc::new(Mutex::new(socket)),
            listener: Arc::new(Mutex::new(listener)),
            listener_port: listener_port,
        })
    }

    pub fn accept(&self) -> io::Result<Connection> {
        let (transport_sender, transport_receiver) = mpsc::channel();
        let protected_tcp_acceptor = self.listener.clone();
        let tcp_acceptor_thread = try!(thread::Builder::new()
                                           .name("Beacon accept TCP listener".to_owned())
                                           .spawn(move || {
                                               let tcp_acceptor = protected_tcp_acceptor.lock()
                                                                                        .unwrap();
                                               match connection::start_tcp_accept(&tcp_acceptor) {
                                                   Ok(transport) => {

                                                       let _ = transport_sender.send(transport);
                                                   }
                                                   Err(e) => {
                                                       let _ = transport_sender.send(Err(e));
                                                   }
                                               };
                                           }));

        let protected_socket = self.socket.clone();
        let guid = self.guid;
        let tcp_port = self.tcp_listener_port;
        let (socket_sender, socket_receiver) = mpsc::channel::<SocketAddr>();
        let udp_listener_thread = try!(thread::Builder::new()
                     .name("Beacon accept UDP listener".to_owned())
                     .spawn(move || -> io::Result<()> {
                         let socket = protected_socket.lock().unwrap();
                         let mut buffer = vec![0u8; MAGIC_SIZE + GUID_SIZE];
                         loop {
                             let (size, source) = try!(socket.recv_from(&mut buffer[..]));
                             if size != MAGIC_SIZE + GUID_SIZE {
                                 continue;
                             }
                             if buffer[0..MAGIC_SIZE] == MAGIC {
                                 // Request for our port
                                 if buffer[MAGIC_SIZE..(MAGIC_SIZE + GUID_SIZE)] == guid {
                                     continue;  // The request is from ourself - don't respond.
                                 }
                                 let sent_size = try!(socket.send_to(&serialise_port(tcp_port),
                                                                     source));
                                 debug_assert!(sent_size == 2);
                                 break;
                             } else if buffer[0..MAGIC_SIZE] == STOP {
                                 // Request to stop
                                 if buffer[MAGIC_SIZE..(MAGIC_SIZE + GUID_SIZE)] == guid &&
                                    util::is_loopback(&SocketAddrExt::ip(&source)) {
                                     // The request is from ourself - stop.
                                     let _ = socket_sender.send(SocketAddr(source));
                                     return Err(io::Error::new(io::ErrorKind::ConnectionAborted,
                                                               "Stopped beacon listener"));
                                 } else {
                                     continue;
                                 }
                             } else {
                                 continue;
                             }
                         }
                         Ok(())
                     }));

        let result = unwrap_result!(udp_listener_thread.join());
        if let Err(e) = result {
            // Connect to the TCP acceptor to allow its thread to join.
            let _ = TcpStream::connect(("127.0.0.1", self.tcp_listener_port));
            let _ = tcp_acceptor_thread.join();
            // Send a ping back to the UDP socket which sent the stop request.
            if let Ok(requester) = socket_receiver.recv() {
                let sent_size = try!(self.socket
                                         .lock()
                                         .unwrap()
                                         .send_to(&[1u8; 1], &*requester));
                debug_assert!(sent_size == 1);
            }
            return Err(e);
        };
        let _ = tcp_acceptor_thread.join();

        match transport_receiver.recv() {
            Ok(transport_res) => transport_res,
            Err(e) => Err(io::Error::new(io::ErrorKind::BrokenPipe, e.description())),
        }
    }

    pub fn stop(guid_and_port: &(GUID, u16)) {
        // Send a UDP message consisting of our GUID with 'stop' as a prefix.
        let mut send_buffer: Vec<_> = From::from(&STOP[..]);
        let guid: Vec<_> = From::from(&guid_and_port.0[..]);
        send_buffer.extend(guid.into_iter());
        let udp_listener_killer = match UdpSocket::bind("0.0.0.0:0") {
            Ok(socket) => socket,
            Err(_) => return (),
        };
        let _ = udp_listener_killer.set_read_timeout(Some(Duration::new(10, 0)));
        // Safe to use unwrap here - this will always parse as a SocketAddr.
        let udp_listener_address = net::SocketAddr::from_str(&format!("127.0.0.1:{}",
                                                                      guid_and_port.1))
                                       .unwrap();
        let _ = udp_listener_killer.send_to(&send_buffer[..], udp_listener_address);
        // Wait for acknowledgement ping.
        let mut buffer = vec![0u8; 1];
        while let Ok((size, source)) = udp_listener_killer.recv_from(&mut buffer[..]) {
            if source == udp_listener_address {
                debug_assert!(size == 1 && buffer[0] == 1u8);
                break;
            } else {
                continue;
            }
        }
    }

    pub fn beacon_port(&self) -> u16 {
        self.socket
            .lock()
            .unwrap()
            .local_addr()
            .map(|address| address.port())
            .unwrap_or(0u16)
    }

    pub fn beacon_guid(&self) -> GUID {
        self.guid
    }
}

pub fn seek_peers(port: u16, guid_to_avoid: Option<GUID>) -> io::Result<Vec<SocketAddr>> {
    // Bind to a UDP socket
    let socket = try!(UdpSocket::bind("0.0.0.0:0"));
    try!(socket.set_broadcast(true));
    let my_udp_port = try!(socket.local_addr()).port();

    // Send a broadcast consisting of our GUID with 'maid' as a prefix.
    let mut send_buffer: Vec<_> = From::from(&MAGIC[..]);
    let guid: Vec<_> = From::from(&guid_to_avoid.unwrap_or([0; GUID_SIZE])[..]);
    send_buffer.extend(guid.into_iter());
    let sent_size = try!(socket.send_to(&send_buffer[..], ("255.255.255.255", port)));
    debug_assert!(sent_size == send_buffer.len());

    // Since Rust doesn't allow the UDP receiver to be stopped gracefully, prepare a random number
    // to send to the UDP receiver as a shutdown signal.
    let shutdown_value = random::<u64>();

    // Start receiving responses to the broadcast
    let (tx, rx) = mpsc::channel::<SocketAddr>();
    let _udp_response_thread = thread::Builder::new()
                                   .name("Beacon seek_peers UDP response".to_owned())
                                   .spawn(move || -> io::Result<()> {
                                       loop {
                                           let mut buffer = [0u8; 8];
                                           let (size, source) = try!(socket.recv_from(&mut buffer));
                                           match size {
                                               // FIXME Use better ways
                                               2usize => {
                                                   // The response is a serialised port
                                                   let _ = tx.send({
                                                       let port = parse_port(&buffer);
                                                       match source {
                                                            net::SocketAddr::V4(a) => {
                                                                SocketAddr(net::SocketAddr::V4(net::SocketAddrV4::new(*a.ip(), port)))
                                                            }
                                                            // FIXME(dirvine) Hanlde ip6 :10/01/2016
                                                            _ => unimplemented!(),
                                                            //                                SocketAddr::V6(a) => {
                                                            //     SocketAddr::V6(SocketAddrV6::new(*a.ip(), port,
                                                            //                                      a.flowinfo(),
                                                            //                                      a.scope_id()))
                                                            // }
                                                       }
                                                   });
                                               }
                                               8usize => {
                                                   // The response is a shutdown signal
                                                   if parse_shutdown_value(&buffer) ==
                                                      shutdown_value &&
                                                      util::is_loopback(&SocketAddrExt::ip(&source)) {
                                                       break;
                                                   } else {
                                                       continue;
                                                   }
                                               }
                                               _ => {
                                                   // The response is invalid
                                                   continue;
                                               }
                                           };
                                       }
                                       Ok(())
                                   });

    // Send the shutdown signal, giving the peers some time to respond first.
    let _shutdown_thread =
        thread::Builder::new()
            .name("Beacon seek_peers UDP shutdown".to_owned())
            .spawn(move || {
                thread::sleep(Duration::from_millis(500));
                let killer_socket = match UdpSocket::bind("0.0.0.0:0") {
                    Ok(socket) => socket,
                    Err(_) => return (),
                };
                let _ = killer_socket.send_to(&serialise_shutdown_value(shutdown_value),
                                              ("127.0.0.1", my_udp_port));
            });

    // Gather the results.
    let mut result = Vec::<SocketAddr>::new();
    while let Ok(socket_addr) = rx.recv() {
        result.push(socket_addr)
    }
    Ok(result)
}



#[cfg(test)]
mod test {
    use super::*;
    use std::thread;
    use std::net;
    use std::str::FromStr;
    use endpoint::{Protocol, Endpoint};
    use transport;
    use transport::{Message, Handshake};
    use socket_addr::SocketAddr;

    #[test]
    fn test_beacon() {
        let acceptor = unwrap_result!(BroadcastAcceptor::new(0));
        let acceptor_port = acceptor.beacon_port();

        let t1 = thread::Builder::new().name("test_beacon sender".to_owned()).spawn(move || {
            let mut transport = acceptor.accept().unwrap().1;
            unwrap_result!(transport.sender
                                    .send(&Message::UserBlob("hello beacon"
                                                                 .to_owned()
                                                                 .into_bytes())));
        });

        let t2 = thread::Builder::new().name("test_beacon receiver".to_owned()).spawn(move || {
            let endpoint = unwrap_result!(seek_peers(acceptor_port, None))[0];
            let transport =
                unwrap_result!(transport::connect(Endpoint::from_socket_addr(Protocol::Tcp,
                                                                             endpoint)));
            let dummy_handshake = Handshake {
                mapper_port: None,
                external_addr: None,
                remote_addr: SocketAddr(net::SocketAddr::from_str("0.0.0.0:0").unwrap()),
            };
            let (_, mut transport) =
                unwrap_result!(transport::exchange_handshakes(dummy_handshake, transport));

            let msg = unwrap_result!(transport.receiver.receive());
            let msg = unwrap_result!(String::from_utf8(match msg {
                Message::UserBlob(msg) => msg,
                _ => panic!("Wrong message type"),
            }));
            assert_eq!(msg, "hello beacon");
        });

        let t1 = unwrap_result!(t1);
        let t2 = unwrap_result!(t2);
        unwrap_result!(t1.join());
        unwrap_result!(t2.join());
    }

    #[test]
    fn test_avoid_beacon() {
        let acceptor = unwrap_result!(BroadcastAcceptor::new(0));
        let acceptor_port = acceptor.beacon_port();
        let my_guid = acceptor.guid.clone();

        let t1 = thread::Builder::new()
                     .name("test_avoid_beacon acceptor".to_owned())
                     .spawn(move || {
                         let _ = unwrap_result!(acceptor.accept());
                     });

        let t2 = thread::Builder::new()
                     .name("test_avoid_beacon seek_peers 1".to_owned())
                     .spawn(move || {
                         assert!(unwrap_result!(seek_peers(acceptor_port, Some(my_guid)))
                                     .is_empty());
                     });

        // This one is just so that the first thread breaks.
        let t3 = thread::Builder::new()
                     .name("test_avoid_beacon seek_peers 2".to_owned())
                     .spawn(move || {
                         thread::sleep(::std::time::Duration::from_millis(700));
                         let endpoint = unwrap_result!(seek_peers(acceptor_port, None))[0];
                         let transport = unwrap_result!(transport::connect(Endpoint::from_socket_addr(Protocol::Tcp, endpoint)));
                         let dummy_handshake = Handshake {
                             mapper_port: None,
                             external_addr: None,
                             remote_addr: SocketAddr(net::SocketAddr::from_str("0.0.0.0:0").unwrap()),
                         };
                         let _ = unwrap_result!(transport::exchange_handshakes(dummy_handshake, transport));
                     });

        let t1 = unwrap_result!(t1);
        let t2 = unwrap_result!(t2);
        let t3 = unwrap_result!(t3);
        unwrap_result!(t1.join());
        unwrap_result!(t2.join());
        unwrap_result!(t3.join());
    }
}