use crossbeam_channel::{select_biased, Receiver, Sender};
use rand::{self, Rng};
use std::collections::{HashMap, HashSet};
use wg_2024::controller::{DroneCommand, DroneEvent};
use wg_2024::drone::Drone;
use wg_2024::network::SourceRoutingHeader;
use wg_2024::packet::{FloodRequest, FloodResponse, Nack, NackType, NodeType, PacketType};
use wg_2024::{network::NodeId, packet::Packet};

pub struct GameOfDrones {
    pub id: NodeId,
    pub controller_send: Sender<DroneEvent>,
    pub controller_recv: Receiver<DroneCommand>,
    pub packet_recv: Receiver<Packet>,
    pub packet_send: HashMap<NodeId, Sender<Packet>>,
    pub pdr: f32,
    pub flood_ids: HashSet<u64>,
}

impl Drone for GameOfDrones {
    fn new(
        id: NodeId,
        controller_send: Sender<DroneEvent>,
        controller_recv: Receiver<DroneCommand>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
        pdr: f32,
    ) -> Self {
        Self{
            id,
            controller_send,
            controller_recv,
            packet_recv,
            packet_send,
            pdr,
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
                            DroneCommand::Crash=>{
                                self.crash_handle();
                                println!("Drone{} has been taken down",self.id);
                                break;
                            },
                            DroneCommand::AddSender(id,sender)=>{
                                self.add_sender(id,sender.clone());
                            },
                            DroneCommand::SetPacketDropRate(pdr)=>{
                                self.set_pdr(pdr);
                            },
                            DroneCommand::RemoveSender(id)=>{
                                self.remove_sender(id);
                            },
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

    // pub fn new(op: DroneOptions) -> Self {
    //     Drone::new(op)
    // }

    //Used in the flood_request_handle to get all the sender without cloning the all hashmap;
    
    fn get_neighbours_id(&self) -> Vec<NodeId> {
        let mut vec: Vec<NodeId> = Vec::new();
        for id in &self.packet_send {
            vec.push(*id.0);
        }
        vec
    }

    /// Below there are the function for packet handling
    

    //Wrapper function, to handle the diffrent packets

    fn forward_packet(&mut self, packet: Packet) /*->Result<(), crossbeam_channel::SendError<Packet>>*/
    {
        match &packet.pack_type {
            PacketType::FloodRequest(f) => {
                self.flood_request_handle(packet.clone(), f.path_trace.clone(), f.flood_id, f.initiator_id);
            },
            PacketType::MsgFragment(f) => {
                //b: check pdr
                self.fragment_handle(packet.clone(), f.fragment_index);
            },
            _ => {
                self.nfa_handle(packet.clone());
            }
        }
    }

    //It checks all the steps stated in the drone protocol paragraph, it uses auxiliary functions to do it.
    //If all checks are passed successfully it send the packet to the next_hop.
    
    fn fragment_handle(&self, mut packet: Packet, fragment_index: u64) {
        //Step 1
        if self.check_unexpected_recipient(&packet, fragment_index) {
            //Step 3
            if self.check_destination_is_drone(&packet, fragment_index) {
                if self.check_error_in_routing(&packet, fragment_index) {
                    //Step 5
                    if self.drop_check() {
                        self.controller_send.send(DroneEvent::PacketDropped(packet.clone())).ok();
                        let nack_type = NackType::Dropped;
                        self.create_nack_n_send(
                            packet.routing_header.hops.clone(),
                            packet.routing_header.hop_index,
                            nack_type,
                            packet.session_id,
                            fragment_index
                        );
                    } else {
                        packet.routing_header.hop_index += 1;
                        let next_hop = packet.routing_header.hops[packet.routing_header.hop_index];
                        self.packet_send
                            .get(&next_hop)
                            .unwrap()
                            .send(packet.clone())
                            .ok();
                        self.controller_send.send(DroneEvent::PacketSent(packet.clone())).ok();
                    }
                }
            }
        }
    }

    //Function that handle flood_request packets, the routing_header is ignored, it checks if the flood_id
    // is already been received and add the drone id and type in the path_trace.
    //If the flood_id is already been received it creates a flood_response which header is comprised of an 
    // hop_index set to 1 and an hops vector that is the path_trace ids reversed.

    fn flood_request_handle(
        &mut self,
        packet: Packet,
        mut path_trace: Vec<(NodeId, NodeType)>,
        flood_id: u64,
        initiator_id: u8
    ) {
        if let Some(_id) = self.flood_ids.get(&flood_id) {
            println!("Flooding already received");
            path_trace.push((self.id, NodeType::Drone));
            self.create_flood_response_n_send(flood_id, path_trace.clone(),packet.session_id);
        } else {
            self.flood_ids.insert(flood_id);
            path_trace.push((self.id, NodeType::Drone));
            if self.get_neighbours_id().len()==1{   
                println!("Just one neighbor");
                self.create_flood_response_n_send(flood_id, path_trace.clone(),packet.session_id);
            } else {
                for sender in self.get_neighbours_id() {
                    if sender != path_trace.clone()[path_trace.clone().len()-2].0{
                        let facket = Packet {
                            pack_type: PacketType::FloodRequest(FloodRequest{flood_id,initiator_id,path_trace: path_trace.clone()}),
                            routing_header: packet.routing_header.clone(),
                            session_id: packet.session_id
                        };
                        self.packet_send.get(&sender).unwrap().send(facket.clone()).ok();
                        self.controller_send.send(DroneEvent::PacketSent(packet.clone())).ok();
                    }
                }
            }
        }
    }

    //Function to handle flood_response packets, as stated in the protocol the routing_header is not to be
    // ignored, but it can't be dropped; so it execute all the steps of the drone protocol paragraph minus the
    // drop part 

    // fn flood_response_handle(&self, mut packet: Packet) {
    //     //Step 1
    //     if self.check_unexpected_recipient(&packet, 0) {
    //         //Step 3
    //         if self.check_destination_is_drone(&packet, 0) {
    //             if self.check_error_in_routing(&packet, 0) {
    //                 //Step 5
    //                 packet.routing_header.hop_index += 1;
    //                 let next_hop = packet.routing_header.hops[packet.routing_header.hop_index];
    //                 self.packet_send
    //                     .get(&next_hop)
    //                     .unwrap()
    //                     .send(packet.clone())
    //                     .ok();
    //                 self.controller_send.send(DroneEvent::PacketSent(packet.clone())).ok();
    //             } else {
    //                 println!("ERROR IN ROUTING");
    //             }
    //         } else {
    //             println!("ERROR IN DEST");
    //         }
    //     } else {
    //         println!("ERROR IN RECIPIENT");
    //     }
    // }

    //Nack and Ack messages are treated the same by the drones so we use just one function
    //TODO!!: probably better to merge flood_response_handle with nack_ack_handle

    fn nfa_handle(&self, mut packet: Packet) {
        if self.check_unexpected_recipient(&packet, 0) {
            if self.check_destination_is_drone(&packet, 0) {
                if self.check_error_in_routing(&packet, 0) {
                    packet.routing_header.hop_index += 1;
                    let next_hop = packet.routing_header.hops[packet.routing_header.hop_index];
                    self.packet_send
                        .get(&next_hop)
                        .unwrap()
                        .send(packet.clone())
                        .ok(); 
                    self.controller_send.send(DroneEvent::PacketSent(packet.clone())).ok();
                } else {
                    self.controller_send.send(DroneEvent::ControllerShortcut(packet.clone())).ok();
                    eprintln!("Error in routing");
                }
            } else {
                eprintln!("Error in destination");
            }
        } else {
            eprintln!("Error in recipient");
        }
    }

    //This function check if the drone is the intended receiver of a packet, if not it calls
    // the create_nack_n_send function to return the nack to the src, it return a boolean to 
    // notify the handle functions to keep going or not.
    
    fn check_unexpected_recipient(&self, packet: &Packet,  fragment_index: u64) -> bool {
        if packet.routing_header.hops[packet.routing_header.hop_index] == self.id {
            true
        } else {
            let nack_type = NackType::UnexpectedRecipient(self.id);
            self.create_nack_n_send(
                packet.routing_header.hops.clone(),
                packet.routing_header.hop_index,
                nack_type,
                packet.session_id,
                fragment_index
            );
            println!("Drone not supposed to received this");
            false
        }
    }

    // Function to check if the drone is the dest of the packet, behavior as the previous function

    fn check_destination_is_drone(&self, packet: &Packet, fragment_index: u64) -> bool {
        if packet.routing_header.hop_index != packet.routing_header.hops.len() {
            true
        } else {
            let nack_type = NackType::DestinationIsDrone;
            self.create_nack_n_send(
                packet.routing_header.hops.clone(),
                packet.routing_header.hop_index,
                nack_type,
                packet.session_id,
                fragment_index
            );
            println!("Drone is not supopsed to be dest");
            false
        }
    }

    // Function to check if the next_hop is actually a neighbour of the drone, behaves as the previous functions

    fn check_error_in_routing(&self, packet: &Packet, fragment_index: u64) -> bool {
        let next_hop = packet.routing_header.hops[packet.routing_header.hop_index+1];
        if self.get_neighbours_id().contains(&next_hop) {
            true
        } else {
            let nack = NackType::ErrorInRouting(next_hop);
            self.create_nack_n_send(
                packet.routing_header.hops.clone(),
                packet.routing_header.hop_index+1,
                nack,
                packet.session_id,
                fragment_index
            );
            false
        }
    }

    //Function to decide if the packet has to be dropped, it generates a random value if the pdr
    // is gt the value it returns true.
    //It's used only in fragment_handle(..) as it's the only packet that can be dropped.

    fn drop_check(&self) -> bool {
        let mut rng = rand::thread_rng();
        let random_value: f32 = rng.gen_range(0.01..1.00); // Generate a random float between 0 and 1.0
        if random_value < self.pdr {
            true
        } else {
            false
        }
    }

    //Function that creates a nack packet to be sent back to the src.
    //It takes hops and reverse it .
    //Then it takes a a NackType created in the handle function based on the error encounterd and with
    // session id, fragment index it creates a Nack to be put in a new packet and then send it.

    fn create_nack_n_send(&self, hops: Vec<u8>, hop_index: usize, nack_type: NackType, session_id: u64, fragment_index: u64) {
        let mut routing_header = SourceRoutingHeader {
            hop_index: 1,
            hops: hops.clone().split_at(hop_index).0.to_vec(),
        };
        println!("{}",hop_index);

        routing_header.hops.reverse();
        let nack  = Nack { fragment_index, nack_type };
        let nack_packet = Packet {
            pack_type: PacketType::Nack(nack),
            routing_header,
            session_id,
        };
        self.packet_send
            .get(&nack_packet.routing_header.hops.clone()[nack_packet.routing_header.hop_index])
            .unwrap()
            .send(nack_packet.clone())
            .ok();
        self.controller_send.send(DroneEvent::PacketSent(nack_packet.clone())).ok();

    }

    //Following the rules of the flooding protocol it does the same as the create_nack_n_send.

    fn create_flood_response_n_send(&self, flood_id: u64, path_trace: Vec<(NodeId, NodeType)>,session_id: u64) {
        let response = FloodResponse {
            flood_id,
            path_trace: path_trace.clone(),
        };
        println!("{:?}",path_trace.clone());
        let mut hops: Vec<u8> = path_trace.clone().into_iter().map(|f| f.0).collect();
        hops.reverse();
        println!("{:?}",hops.clone());
        let packet = Packet {
            pack_type: PacketType::FloodResponse(response),
            routing_header: SourceRoutingHeader { hop_index: 1, hops },
            session_id,
        };
        self.packet_send
            .get(&packet.routing_header.hops[packet.routing_header.hop_index])
            .unwrap()
            .send(packet.clone())
            .ok();
        self.controller_send.send(DroneEvent::PacketSent(packet.clone())).ok();
    }


    /// Below there are the functions for command handling;
    fn crash_handle(&mut self){
        while let Ok(packet) = self.packet_recv.recv(){
            self.forward_packet(packet);
        }
    }
    
    fn add_sender(&mut self, id: NodeId, sender: Sender<Packet>){
        self.packet_send.insert(id, sender).unwrap();
    }
    
    fn set_pdr(&mut self, pdr: f32){
        self.pdr = pdr;
    }

    fn remove_sender(&mut self, id: NodeId) {
        self.packet_send.remove(&id);
    }

}


#[cfg(test)]
mod test {
    use std::collections::HashMap;

    use crossbeam_channel::unbounded;
    use wg_2024::{ controller::{DroneCommand, DroneEvent}, drone::Drone, network::SourceRoutingHeader, packet::{Ack, Packet, PacketType}};

    use crate::GameOfDrones;

    #[test]
    fn test_1 (){
        let (packet_send_1, packet_recv_1) = unbounded::<Packet>();
        let (packet_send_2, packet_recv_2) = unbounded::<Packet>();
        
        let (_command_send_1, command_recv_1) = unbounded::<DroneCommand>();
        let (command_send_2, _command_recv_2) = unbounded::<DroneEvent>();

        let mut drone_op_1 = ( 
            1, 
            command_send_2.clone(),
            command_recv_1.clone(),
            packet_recv_1.clone(),
            HashMap::new(),
            0.05
        );

        drone_op_1.4.insert(2, packet_send_1.clone());

        let mut drone_op_2 =(
            2, 
            command_send_2.clone(),
            command_recv_1.clone(),
            packet_recv_2.clone(),
            HashMap::new(),
            0.05
        );

        drone_op_2.4.insert(1, packet_send_2.clone());

        let drone1 = GameOfDrones::new(drone_op_1.0, drone_op_1.1, drone_op_1.2, drone_op_1.3, drone_op_1.4, drone_op_1.5);
        let drone2 = GameOfDrones::new(drone_op_2.0, drone_op_2.1, drone_op_2.2, drone_op_2.3, drone_op_2.4, drone_op_2.5);

        let pack_type = PacketType::Ack(Ack{fragment_index: 0});
        let packet1 = Packet { pack_type:pack_type.clone(), routing_header: SourceRoutingHeader {hop_index: 3, hops: [3,1,2].to_vec()},session_id: 1};
        assert_eq!(drone2.check_destination_is_drone(&packet1, 0),false);
    }

    #[test]
    fn test_2() {
        let (packet_send_1, packet_recv_1) = unbounded::<Packet>();
        let (packet_send_2, packet_recv_2) = unbounded::<Packet>();
        
        let (_command_send_1, command_recv_1) = unbounded::<DroneCommand>();
        let (command_send_2, _command_recv_2) = unbounded::<DroneEvent>();

        let mut drone_op_1 = ( 
            1, 
            command_send_2.clone(),
            command_recv_1.clone(),
            packet_recv_1.clone(),
            HashMap::new(),
            0.05
        );

        drone_op_1.4.insert(2, packet_send_1.clone());

        let mut drone_op_2 =(
            2, 
            command_send_2.clone(),
            command_recv_1.clone(),
            packet_recv_2.clone(),
            HashMap::new(),
            0.05
        );

        drone_op_2.4.insert(1, packet_send_2.clone());

        let drone1 = GameOfDrones::new(drone_op_1.0, drone_op_1.1, drone_op_1.2, drone_op_1.3, drone_op_1.4, drone_op_1.5);
        let drone2 = GameOfDrones::new(drone_op_2.0, drone_op_2.1, drone_op_2.2, drone_op_2.3, drone_op_2.4, drone_op_2.5);

        let pack_type = PacketType::Ack(Ack{fragment_index: 0});
        let packet1 = Packet { pack_type:pack_type.clone(), routing_header: SourceRoutingHeader {hop_index: 2, hops: [3,2,1].to_vec()},session_id: 1};
        assert_eq!(drone2.check_error_in_routing(&packet1, 0),true);
    }


    #[test]
    fn test_3() {
        let (packet_send_1, packet_recv_1) = unbounded::<Packet>();
        let (packet_send_2, _packet_recv_2) = unbounded::<Packet>();
        
        let (_command_send_1, command_recv_1) = unbounded::<DroneCommand>();
        let (command_send_2, _command_recv_2) = unbounded::<DroneEvent>();

        let mut drone_op_1 = ( 
            1, 
            command_send_2.clone(),
            command_recv_1.clone(),
            packet_recv_1.clone(),
            HashMap::new(),
            0.05
        );

        drone_op_1.4.insert(2, packet_send_1.clone());

        let mut drone_op_2 =(
            2, 
            command_send_2.clone(),
            command_recv_1.clone(),
            packet_recv_1.clone(),
            HashMap::new(),
            0.05
        );

        drone_op_2.4.insert(1, packet_send_2.clone());

        let drone1 = GameOfDrones::new(drone_op_1.0, drone_op_1.1, drone_op_1.2, drone_op_1.3, drone_op_1.4, drone_op_1.5);
        let drone2 = GameOfDrones::new(drone_op_2.0, drone_op_2.1, drone_op_2.2, drone_op_2.3, drone_op_2.4, drone_op_2.5);

        let pack_type = PacketType::Ack(Ack{fragment_index: 0});
        let packet1 = Packet { pack_type:pack_type.clone(), routing_header: SourceRoutingHeader {hop_index: 3, hops: [3,1,2].to_vec()},session_id: 1};
        assert_eq!(drone2.check_destination_is_drone(&packet1, 0),false);
    }
}