use std::thread::{self, spawn, JoinHandle};
use std::time::TryFromFloatSecsError;
use std::{collections::HashMap, hash::Hash};
use crossbeam_channel::{Sender,Receiver,select, unbounded};
use wg_2024::config::Config;
use wg_2024::packet::PacketType;
use wg_2024::{network::NodeId, packet::Packet};
use wg_2024::drone::{Drone, DroneOptions};
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
            packet_send: HashMap::new(),
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
                            //PacketType::FloodRequest(_) => unimplemented!(),
                            //PacketType::FloodResponse(_) => unimplemented!(),
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

    pub fn forward_packet(&self,packet: Packet,rec_id: NodeId/* other data?? */)->bool{
        if let Some(sender) = self.packet_send.get_key_value(&rec_id){
            sender.1.send(packet);
            return true;
        }
        false
    }

    pub fn show_data(&self){  //show own data + neighbours + packet status in own comms channel ??
        if self.packet_recv.recv().is_ok(){
            println!("OK");
        } else {
            println!("ERR");
        }
    }

    pub fn release_channels(&self){
        // we discussed about how a drone that received a crash command should behave
        // I don't know how, but the comm channels of that drone should be empty otherwise we'll lose data
        // so I dont' know if it's purely a drone api but for know let's put it here.
        unimplemented!()
    }

    

}

pub fn populate_channels(){
    let config: wg_2024::config::Config = todo!();

    // these hashmaps can then be stored in the simulation controller
    let mut packet_channels: HashMap<NodeId, (Sender<Packet>, Receiver<Packet>)> = HashMap::new();
    let mut command_channels: HashMap<NodeId, (Sender<Command>, Receiver<Command>)> =
        HashMap::new();

    let mut join_handles: Vec<JoinHandle<()>> = Vec::new();

    //
    // since the config doesn't use NodeId but u64 in this branch, you'll see conversions that won't be needed in the future
    //

    for drone in config.drone.iter() {
        //create unbounded channel for drones
        packet_channels.insert(drone.id as NodeId, unbounded::<Packet>());
        command_channels.insert(drone.id as NodeId, unbounded::<Command>());
    }

    for drone in config.drone.iter() {
        //clones all the sender channels for the connected drones
        let mut packet_send: HashMap<NodeId, Sender<Packet>> = HashMap::new();

        for connected_drone in drone.connected_drone_ids.iter() {
            packet_send.insert(
                *connected_drone as NodeId,
                packet_channels
                    .get(&(*connected_drone as NodeId))
                    .unwrap()
                    .0
                    .clone(),
            );
        }

        // clone the channels to give them to each thread
        let packet_recv = packet_channels.get(&(drone.id as u8)).unwrap().1.clone();
        let sim_contr_recv = command_channels.get(&(drone.id as u8)).unwrap().1.clone();
        let sim_contr_send = command_channels.get(&(drone.id as u8)).unwrap().0.clone();

        // since the thread::spawn function will take ownership of the values, we need to copy or clone the values from 'drone' since it's a borrow
        let id: NodeId = drone.id.try_into().unwrap();
        let pdr = drone.pdr as f32;



        join_handles.push(thread::spawn(move || {
            let mut drone = GameOfDrones::new(DroneOptions {
                id,
                sim_contr_recv,
                sim_contr_send,
                packet_recv,
                pdr,
                //packet_send,
            });

            drone.run();
        }));


    }

    // here you'd create your simulation controller and also pass all the channels to it

    // joining behaviour needs to be refined
    join_handles[0].join().ok();
}


