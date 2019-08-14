//! The push thread.
//!
//! This is the thread for distributing rumors to members. It distributes to `FANOUT` members, no
//! more often than `Timing::GOSSIP_PERIOD_DEFAULT_MS`.

use crate::{member::{Member,
                     Membership},
            rumor::{RumorEnvelope,
                    RumorKey,
                    RumorKind,
                    RumorType},
            server::{timing::Timing,
                     Server},
            trace::TraceKind,
            ZMQ_CONTEXT};
use habitat_common::liveliness_checker;
use habitat_core::util::ToI64;
use prometheus::{IntCounterVec,
                 IntGaugeVec};
use std::{thread,
          time::Duration};
use time::SteadyTime;
use zmq;

const FANOUT: usize = 5;

lazy_static! {
    static ref GOSSIP_MESSAGES_SENT: IntCounterVec =
        register_int_counter_vec!("hab_butterfly_gossip_messages_sent_total",
                                  "Total number of gossip messages sent",
                                  &["type", "mode"]).unwrap();
    static ref GOSSIP_BYTES_SENT: IntGaugeVec =
        register_int_gauge_vec!("hab_butterfly_gossip_sent_bytes",
                                "Gossip message size sent in bytes",
                                &["type", "mode"]).unwrap();
}

pub fn spawn_thread(name: String, server: Server, timing: Timing) -> std::io::Result<()> {
    thread::Builder::new().name(name)
                          .spawn(move || -> ! { run_loop(&server, &timing) })
                          .map(|_| ())
}

/// Executes the Push thread. Gets a list of members to talk to that are not Confirmed; then
/// proceeds to process the list in `FANOUT` sized chunks. If we finish sending the messages to
/// all FANOUT targets faster than `Timing::GOSSIP_PERIOD_DEFAULT_MS`, we will block until we
/// exceed that time.
fn run_loop(server: &Server, timing: &Timing) -> ! {
    loop {
        liveliness_checker::mark_thread_alive().and_divergent();

        if server.paused() {
            thread::sleep(Duration::from_millis(100));
            continue;
        }

        server.update_gossip_round();

        let mut check_list = server.member_list.check_list_mlr(server.member_id());
        let long_wait = timing.gossip_timeout();

        'fanout: loop {
            let mut thread_list = Vec::with_capacity(FANOUT);
            if check_list.is_empty() {
                break 'fanout;
            }
            let drain_length = if check_list.len() >= FANOUT {
                FANOUT
            } else {
                check_list.len()
            };
            let next_gossip = timing.gossip_timeout();
            for member in check_list.drain(0..drain_length) {
                if server.is_member_blocked(&member.id) {
                    debug!("Not sending rumors to {} - it is blocked", member.id);
                    continue;
                }
                // Unlike the SWIM mechanism, we don't actually want to send gossip traffic to
                // persistent members that are confirmed dead. When the failure detector thread
                // finds them alive again, we'll go ahead and get back to the business at hand.
                if server.member_list.pingable_mlr(&member)
                   && !server.member_list.persistent_and_confirmed_mlr(&member)
                {
                    let rumors = server.rumor_keys();
                    if !rumors.is_empty() {
                        let sc = server.clone();
                        let guard = match thread::Builder::new().name(String::from("push-worker"))
                                                                .spawn(move || {
                                                                    send_rumors_rsr_mlr(&sc,
                                                                                        &member,
                                                                                        &rumors)
                                                                }) {
                            Ok(guard) => guard,
                            Err(e) => {
                                error!("Could not spawn thread: {}", e);
                                continue;
                            }
                        };
                        thread_list.push(guard);
                    }
                }
            }
            let num_threads = thread_list.len();
            for guard in thread_list.drain(0..num_threads) {
                let _ = guard.join()
                             .map_err(|e| error!("Push worker died: {:?}", e));
            }
            if SteadyTime::now() < next_gossip {
                let wait_time = (next_gossip - SteadyTime::now()).num_milliseconds();
                if wait_time > 0 {
                    thread::sleep(Duration::from_millis(wait_time as u64));
                }
            }
        }
        if SteadyTime::now() < long_wait {
            let wait_time = (long_wait - SteadyTime::now()).num_milliseconds();
            if wait_time > 0 {
                thread::sleep(Duration::from_millis(wait_time as u64));
            }
        }
    }
}

/// Send the list of rumors to a given member. This method creates an outbound socket and then
/// closes the connection as soon as we are done sending rumors. ZeroMQ may choose to keep the
/// connection and socket open for 1 second longer - so it is possible, but unlikely, that this
/// method can lose messages.
///
/// # Locking (see locking.md)
/// * `RumorStore::list` (read)
/// * `MemberList::entries` (read)
// If we ever need to modify this function, it would be an excellent opportunity to
// simplify the redundant aspects and remove this allow(clippy::cognitive_complexity),
// but changing it in the absence of other necessity seems like too much risk for the
// expected reward.
#[allow(clippy::cognitive_complexity)]
fn send_rumors_rsr_mlr(server: &Server, member: &Member, rumors: &[RumorKey]) {
    let socket = (**ZMQ_CONTEXT).as_mut()
                                .socket(zmq::PUSH)
                                .expect("Failure to create the ZMQ push socket");
    socket.set_linger(1000)
          .expect("Failure to set the ZMQ push socket to not linger");
    socket.set_tcp_keepalive(0)
          .expect("Failure to set the ZMQ push socket to not use keepalive");
    socket.set_immediate(true)
          .expect("Failure to set the ZMQ push socket to immediate");
    socket.set_sndhwm(1000)
          .expect("Failure to set the ZMQ push socket hwm");
    socket.set_sndtimeo(500)
          .expect("Failure to set the ZMQ send timeout");
    let to_addr = format!("{}:{}", member.address, member.gossip_port);

    match socket.connect(&format!("tcp://{}", to_addr)) {
        Ok(()) => debug!("Connected push socket to {:?}", member),
        Err(e) => {
            error!("Cannot connect push socket to {:?}: {:?}", member, e);
            let label_values = &["socket_connect", "failure"];
            GOSSIP_MESSAGES_SENT.with_label_values(label_values).inc();
            GOSSIP_BYTES_SENT.with_label_values(label_values).set(0);
            return;
        }
    }

    'rumorlist: for rumor_key in rumors.iter() {
        let rumor_as_bytes = match rumor_key.kind {
            RumorType::Member => {
                let send_rumor = match create_member_rumor_mlr(&server, &rumor_key) {
                    Some(rumor) => rumor,
                    None => continue 'rumorlist,
                };
                trace_it!(GOSSIP: &server,
                          TraceKind::SendRumor,
                          &member.id,
                          &send_rumor);
                match send_rumor.encode() {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        error!("Could not write our own rumor to bytes; abandoning sending \
                                rumor: {:?}",
                               e);
                        let label_values = &["member_rumor_encode", "failure"];
                        GOSSIP_MESSAGES_SENT.with_label_values(label_values).inc();
                        GOSSIP_BYTES_SENT.with_label_values(label_values).set(0);
                        continue 'rumorlist;
                    }
                }
            }
            RumorType::Service => {
                match server.service_store.lock_rsr().encode_rumor_for(&rumor_key) {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        error!("Could not write our own rumor to bytes; abandoning sending \
                                rumor: {:?}",
                               e);
                        let label_values = &["service_rumor_encode", "failure"];
                        GOSSIP_MESSAGES_SENT.with_label_values(label_values).inc();
                        GOSSIP_BYTES_SENT.with_label_values(label_values).set(0);
                        continue 'rumorlist;
                    }
                }
            }
            RumorType::ServiceConfig => {
                match server.service_config_store
                            .lock_rsr()
                            .encode_rumor_for(&rumor_key)
                {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        error!("Could not write our own rumor to bytes; abandoning sending \
                                rumor: {:?}",
                               e);
                        let label_values = &["service_config_rumor_encode", "failure"];
                        GOSSIP_MESSAGES_SENT.with_label_values(label_values).inc();
                        GOSSIP_BYTES_SENT.with_label_values(label_values).set(0);
                        continue 'rumorlist;
                    }
                }
            }
            RumorType::ServiceFile => {
                match server.service_file_store
                            .lock_rsr()
                            .encode_rumor_for(&rumor_key)
                {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        error!("Could not write our own rumor to bytes; abandoning sending \
                                rumor: {:?}",
                               e);
                        let label_values = &["service_file_rumor_encode", "failure"];
                        GOSSIP_MESSAGES_SENT.with_label_values(label_values).inc();
                        GOSSIP_BYTES_SENT.with_label_values(label_values).set(0);
                        continue 'rumorlist;
                    }
                }
            }
            RumorType::Departure => {
                match server.departure_store
                            .lock_rsr()
                            .encode_rumor_for(&rumor_key)
                {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        error!("Could not write our own rumor to bytes; abandoning sending \
                                rumor: {:?}",
                               e);
                        let label_values = &["departure_rumor_encode", "failure"];
                        GOSSIP_MESSAGES_SENT.with_label_values(label_values).inc();
                        GOSSIP_BYTES_SENT.with_label_values(label_values).set(0);
                        continue 'rumorlist;
                    }
                }
            }
            RumorType::Election => {
                match server.election_store
                            .lock_rsr()
                            .encode_rumor_for(&rumor_key)
                {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        error!("Could not write our own rumor to bytes; abandoning sending \
                                rumor: {:?}",
                               e);
                        let label_values = &["election_rumor_encode", "failure"];
                        GOSSIP_MESSAGES_SENT.with_label_values(label_values).inc();
                        GOSSIP_BYTES_SENT.with_label_values(label_values).set(0);
                        continue 'rumorlist;
                    }
                }
            }
            RumorType::ElectionUpdate => {
                match server.update_store.lock_rsr().encode_rumor_for(&rumor_key) {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        error!("Could not write our own rumor to bytes; abandoning sending \
                                rumor: {:?}",
                               e);
                        let label_values = &["election_update_rumor_encode", "failure"];
                        GOSSIP_MESSAGES_SENT.with_label_values(label_values).inc();
                        GOSSIP_BYTES_SENT.with_label_values(label_values).set(0);
                        continue 'rumorlist;
                    }
                }
            }
            RumorType::Fake | RumorType::Fake2 => {
                debug!("You have fake rumors; how odd!");
                continue 'rumorlist;
            }
        };
        let rumor_len = rumor_as_bytes.len().to_i64();
        let payload = match server.generate_wire(rumor_as_bytes) {
            Ok(payload) => payload,
            Err(e) => {
                error!("Generating protobuf failed: {}", e);
                let label_values = &["generate_wire", "failure"];
                GOSSIP_MESSAGES_SENT.with_label_values(label_values).inc();
                GOSSIP_BYTES_SENT.with_label_values(label_values)
                                 .set(rumor_len);
                continue 'rumorlist;
            }
        };
        match socket.send(&payload, 0) {
            Ok(()) => {
                GOSSIP_MESSAGES_SENT.with_label_values(&[&rumor_key.kind.to_string(), "success"])
                                    .inc();
                GOSSIP_BYTES_SENT.with_label_values(&[&rumor_key.kind.to_string(), "success"])
                                 .set(payload.len().to_i64());
                debug!("Sent rumor {:?} to {:?}", rumor_key, member);
            }
            Err(e) => {
                warn!("Could not send rumor to {:?} @ {:?}; ZMQ said: {:?}",
                      member.id, to_addr, e)
            }
        }
    }
}

/// Given a rumorkey, creates a protobuf rumor for sharing.
///
/// # Locking (see locking.md)
/// * `MemberList::entries` (read)
fn create_member_rumor_mlr(server: &Server, rumor_key: &RumorKey) -> Option<RumorEnvelope> {
    let mut member = server.member_list.get_cloned_mlr(&rumor_key.to_string())?;

    // We should refresh our own Member rumor before we send it out
    if member.id == server.member_id() {
        member.refresh();
    }

    let payload = Membership { member,
                               health: server.member_list
                                             .health_of_by_id_mlr(&rumor_key.to_string())
                                             .unwrap() };
    let rumor = RumorEnvelope { r#type:  RumorType::Member,
                                from_id: server.member_id().to_string(),
                                kind:    RumorKind::Membership(payload), };
    Some(rumor)
}
