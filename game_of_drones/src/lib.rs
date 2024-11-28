use crossbeam_channel::{select_biased, Receiver, Sender};
use rand::{self, Rng};
use std::collections::{HashMap, HashSet};
use wg_2024::controller::{DroneCommand, NodeEvent};
use wg_2024::drone::{Drone, DroneOptions};
use wg_2024::network::SourceRoutingHeader;
use wg_2024::packet::{FloodResponse, Nack, NodeType, PacketType};
use wg_2024::{network::NodeId, packet::Packet};

pub struct GameOfDrones {
    pub id: NodeId,
    pub controller_send: Sender<NodeEvent>,
    pub controller_recv: Receiver<DroneCommand>,
    pub packet_recv: Receiver<Packet>,
    pub packet_send: HashMap<NodeId, Sender<Packet>>,
    pub pdr: f32,
    pub flood_ids: HashSet<u64>,
}

impl Drone for GameOfDrones {
    fn new(options: DroneOptions) -> Self {
        Self {
            id: options.id,
            controller_send: options.controller_send,
            controller_recv: options.controller_recv,
            packet_recv: options.packet_recv,
            packet_send: options.packet_send,
            pdr: options.pdr,
            flood_ids: HashSet::new(),
        }
    }

    fn run(&mut self) {
        self.run_internal();
    }
}

impl GameOfDrones {
    fn run_internal(&mut self) {
        loop {
            select_biased! {
                recv(self.controller_recv) -> command_res => {
                    if let Ok(command) = command_res {
                        match command {
                            DroneCommand::Crash=>{},
                            DroneCommand::AddSender(_id,_sender)=>{},
                            DroneCommand::SetPacketDropRate(_pdr)=>{}
                        }
                    }
                },
                recv(self.packet_recv) -> packet_res => {
                    if let Ok(packet) = packet_res {
                        self.forward_packet(packet);
                    }
                },
            }
        }
    }

    pub fn new(op: DroneOptions) -> Self {
        Drone::new(op)
    }

    pub fn get_neighbours_id(&self) -> Vec<NodeId> {
        let mut vec: Vec<NodeId> = Vec::new();
        for id in &self.packet_send {
            vec.push(*id.0);
        }
        vec
    }

    fn forward_packet(&self, packet: Packet) /*->Result<(), crossbeam_channel::SendError<Packet>>*/
    {
        match &packet.pack_type {
            PacketType::FloodRequest(f) => {
                self.flood_request_handle(packet.clone(), f.path_trace.clone(), f.flood_id);
            }
            PacketType::FloodResponse(_f) => {
                self.flood_response_handle(packet.clone());
            }
            PacketType::MsgFragment(f) => {
                //b: check pdr
                self.fragment_handle(packet.clone(), f.fragment_index);
            }
            _ => {
                self.nack_ack_handle(packet.clone());
            }
        }
    }

    
    fn fragment_handle(&self, mut packet: Packet, fragment_index: u64) {
        //Step 1
        if self.check_unexpected_recipient(&packet) {
            //Step 3
            if self.check_destination_is_drone(&packet) {
                if self.check_error_in_routing(&packet) {
                    //Step 5
                    if self.drop_check() {
                        let nack = Nack::Dropped(fragment_index);
                        self.create_nack_n_send(
                            packet.routing_header.hops.clone(),
                            packet.routing_header.hop_index,
                            nack,
                            packet.session_id,
                        );
                    } else {
                        packet.routing_header.hop_index += 1;
                        let next_hop = packet.routing_header.hops[packet.routing_header.hop_index];
                        self.packet_send
                            .get(&next_hop)
                            .unwrap()
                            .send(packet.clone())
                            .ok();
                    }
                }
            }
        }
    }

    fn flood_request_handle(
        &self,
        packet: Packet,
        mut path_trace: Vec<(NodeId, NodeType)>,
        flood_id: u64,
    ) {
        if let Some(_id) = self.flood_ids.get(&flood_id) {
            path_trace.push((self.id, NodeType::Drone));
            self.create_flood_response_n_send(flood_id, path_trace.clone());
        } else {
            path_trace.push((self.id, NodeType::Drone));
            if self.get_neighbours_id().is_empty(){
                self.create_flood_response_n_send(flood_id, path_trace.clone());
            } else {
                for sender in self.get_neighbours_id() {
                    self.packet_send.get(&sender).unwrap().send(packet.clone()).ok();
                }
            }
        }
    }

    fn flood_response_handle(&self, mut packet: Packet) {
        //Step 1
        if self.check_unexpected_recipient(&packet) {
            //Step 3
            if self.check_destination_is_drone(&packet) {
                if self.check_error_in_routing(&packet) {
                    //Step 5
                    packet.routing_header.hop_index += 1;
                    let next_hop = packet.routing_header.hops[packet.routing_header.hop_index];
                    self.packet_send
                        .get(&next_hop)
                        .unwrap()
                        .send(packet.clone())
                        .ok();
                }
            }
        }
    }

    fn nack_ack_handle(&self, mut packet: Packet) {
        //Step 1
        if self.check_unexpected_recipient(&packet) {
            //Step 3
            if self.check_destination_is_drone(&packet) {
                if self.check_error_in_routing(&packet) {
                    packet.routing_header.hop_index += 1;
                    let next_hop = packet.routing_header.hops[packet.routing_header.hop_index];
                    self.packet_send
                        .get(&next_hop)
                        .unwrap()
                        .send(packet.clone())
                        .ok();
                }
            }
        }
    }
    
    fn check_unexpected_recipient(&self, packet: &Packet) -> bool {
        if packet.routing_header.hops[packet.routing_header.hop_index] == self.id {
            true
        } else {
            let nack = Nack::UnexpectedRecipient(self.id);
            self.create_nack_n_send(
                packet.routing_header.hops.clone(),
                packet.routing_header.hop_index,
                nack,
                packet.session_id,
            );
            println!("Drone not supposed to received this");
            false
        }
    }

    fn check_destination_is_drone(&self, packet: &Packet) -> bool {
        if packet.routing_header.hop_index != packet.routing_header.hops.len() - 1 {
            true
        } else {
            let nack: Nack = Nack::DestinationIsDrone;
            self.create_nack_n_send(
                packet.routing_header.hops.clone(),
                packet.routing_header.hop_index,
                nack,
                packet.session_id,
            );
            println!("Drone is not supopsed to be dest");
            false
        }
    }

    fn check_error_in_routing(&self, packet: &Packet) -> bool {
        let next_hop = packet.routing_header.hops[packet.routing_header.hop_index];
        if self.get_neighbours_id().contains(&next_hop) {
            true
        } else {
            let nack: Nack = Nack::ErrorInRouting(next_hop);
            self.create_nack_n_send(
                packet.routing_header.hops.clone(),
                packet.routing_header.hop_index,
                nack,
                packet.session_id,
            );
            false
        }
    }

    fn drop_check(&self) -> bool {
        let mut rng = rand::thread_rng();
        let random_value: f32 = rng.gen_range(0.01..1.00); // Generate a random float between 0 and 1.0
        if random_value > self.pdr {
            true
        } else {
            false
        }
    }

    fn create_nack_n_send(&self, hops: Vec<u8>, hop_index: usize, nack: Nack, session_id: u64) {
        let mut routing_header = SourceRoutingHeader {
            hop_index: 1,
            hops: hops.clone().split_at(hop_index).0.to_vec(),
        };
        routing_header.hops.reserve(routing_header.hops.len());
        let nack_packet = Packet {
            pack_type: PacketType::Nack(nack),
            routing_header,
            session_id,
        };
        self.packet_send
            .get(&nack_packet.routing_header.hops.clone()[nack_packet.routing_header.hop_index])
            .unwrap()
            .send(nack_packet)
            .ok();
    }

    fn create_flood_response_n_send(&self, flood_id: u64, path_trace: Vec<(NodeId, NodeType)>) {
        let response = FloodResponse {
            flood_id,
            path_trace: path_trace.clone(),
        };
        let mut hops: Vec<u8> = path_trace.clone().into_iter().map(|f| f.0).collect();
        hops.reverse();
        let packet = Packet {
            pack_type: PacketType::FloodResponse(response),
            routing_header: SourceRoutingHeader { hop_index: 1, hops },
            session_id: 1,
        };
        self.packet_send
            .get(&packet.routing_header.hops[packet.routing_header.hop_index])
            .unwrap()
            .send(packet)
            .ok();
    }
}
