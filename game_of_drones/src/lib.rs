use std::collections::HashMap;
use crossbeam_channel::{Sender,Receiver,select};
use wg_2024::packet::PacketType;
use wg_2024::{network::NodeId, packet::Packet};
use wg_2024::drone::{Drone,DroneOptions};
use wg_2024::controller::Command;


pub struct GameOfDrones {
    pub id: NodeId,
    pub sim_contr_send: Sender<Command>,
    pub sim_contr_recv: Receiver<Command>,
    pub packet_recv: Receiver<Packet>,
    pub packet_send: HashMap<NodeId,Sender<Packet>>,
    pub pdr: f32,
}

impl Drone for GameOfDrones {
    fn new(options: DroneOptions) -> Self {

        Self { 
            id: options.id, 
            sim_contr_send: options.sim_contr_send, 
            sim_contr_recv: options.sim_contr_recv,
            packet_recv: options.packet_recv,
            packet_send: options.packet_send,
            pdr: options.pdr
        }
    }

    fn run(&mut self) {
        self.run_internal();
    }
}

impl GameOfDrones {

    fn run_internal(&mut self) {
        loop {
            select! {
                recv(self.packet_recv) -> packet_res => {
                    if let Ok(packet) = packet_res {    
                        // each match branch may call a function to handle it to make it more readable
                        match packet.pack_type {
                            PacketType::Nack(_nack) => unimplemented!(),
                            PacketType::Ack(_ack) => unimplemented!(),
                            PacketType::MsgFragment(_fragment) => unimplemented!(),
                            PacketType::FloodRequest(_) => unimplemented!(),
                            PacketType::FloodResponse(_) => unimplemented!(),
                        }
                    }
                },
                recv(self.sim_contr_recv) -> command_res => {
                    if let Ok(_command) = command_res {
                        // handle the simulation controller's command
                    }
                }
            }
        }
    }

    pub fn new(op: DroneOptions)->Self{
        Drone::new(op)
    }

    pub fn get_neighbours_id(&self)->Vec<NodeId>{
        let mut vec: Vec<NodeId> = Vec::new();
        for id in &self.packet_send {
            vec.push(*id.0);
        }
        vec
    }

    pub fn forward_packet(&self,mut packet: Packet,rec_id: NodeId/* other data?? */)->Result<(), crossbeam_channel::SendError<Packet>>{
        packet.routing_header.hop_index+=1;
        match self.packet_send.get(&rec_id).unwrap().send(packet) {
            Ok(())=>{
                Ok(())
            },
            Err(e)=>{
                Err(e)
            }
        }
    }

    pub fn show_data(&self){  //show own data + neighbours + packet status in comms channels ??
    }

    pub fn release_channels(&self){
        // we discussed about how a drone that received a crash command should behave
        // I don't know how, but the comm channels of that drone should be empty otherwise we'll lose data
        // so I dont' know if it's purely a drone api but for know let's put it here.
        unimplemented!()
    }
}