/*
Darkhill(Mørkhøj) Synthesizer
Til SRP projekt

Af Kaare G. S. Hansen - marts 2020

Baseret på eksempel fra Portmidi
*/


extern crate portmidi as pm;
extern crate rustc_serialize;
extern crate docopt;
extern crate anyhow;
extern crate cpal;

use cpal::traits::{DeviceTrait, EventLoopTrait, HostTrait};
use std::time::Duration;
use std::thread;
use std::sync::mpsc;
use std::sync::Mutex;
use std::sync::Arc;
//use orbtk::prelude::*;
//use druid::widget::{Button, Flex, Label};
//use druid::{AppLauncher, LocalizedString, Widget, WidgetExt, WindowDesc};


const USAGE: &'static str = r#"
Darkhill synthesizer - Af Kaare G. S. Hansen

portmidi-rs: monitor-device example

Usage:
    monitor-device <device-id>

Options:
    -h --help   Show this screen.

Omitting <device-id> will list the available devices.
"#;

#[derive(Debug, RustcDecodable)]
struct Args {
    arg_device_id: i32,
}

fn print_devices(pm: &pm::PortMidi) {
    for dev in pm.devices().unwrap() {
        println!("{}", dev);
    }
}

#[derive(Copy, Clone)]
struct Voice {
    freq: f32,
    midi_note: u8,
    amplitude: f32,
    time_attack: f32,
    time_decay: f32,
    time: f32,
    active: bool,
    released: bool,
}

struct ReverbBuffer {
    buffer: Vec<f32>,
    pos: usize,
}

impl ReverbBuffer {
    fn new(n: usize) -> ReverbBuffer {
        let buffer = vec![0.0; n];
        let pos = 0;
        ReverbBuffer {buffer: buffer, pos: pos}
    } 
    
    fn increment(&mut self) {
        self.pos = self.pos + 1;
        if(self.pos >= self.buffer.len()) {
            self.pos = 0;
        }
    }
}


enum Instrument {
    Orgel,
    Weird,
    Brass,
    SoftSaw,
    HardSaw,
}


fn main() -> Result<(), anyhow::Error> {
    let host = cpal::default_host();
    let device = host.default_output_device().expect("failed to find a default output device");
    let format = device.default_output_format()?;
    let event_loop = host.event_loop();
    let stream_id = event_loop.build_output_stream(&device, &format)?;
    event_loop.play_stream(stream_id.clone())?;
    
    let context = pm::PortMidi::new().unwrap();
    let timeout = Duration::from_millis(1);

    let mut args: Args = docopt::Docopt::new(USAGE).and_then(|d| d.decode()).unwrap_or_else(|err| {
        print_devices(&context);
        err.exit();
    });

    let mut instrument: Instrument = Instrument::HardSaw;

    args.arg_device_id = 0;

    let info = context.device(args.arg_device_id).unwrap();
    println!("Listening on: {}) {}", info.id(), info.name());

    let in_port = context.input_port(info, 1024).unwrap();
    
    let (tx, rx) = mpsc::channel();
    let midi_thread = thread::spawn(move|| {
        while let Ok(_) = in_port.poll() {
            if let Ok(Some(events)) = in_port.read_n(1024) {    
                for e in events {
                    tx.send(e).unwrap();
                }    
            }
            thread::sleep(timeout);
        }
    });
    
    let sample_rate = format.sample_rate.0 as f32;
    let mut sample_clock = 0f32;
    let mut voice_n = 0;
    
    let mut voices: [Voice; 10] = [Voice{freq:0.0,midi_note:0,amplitude:0.0,time_attack:0.0,time_decay:0.0,time:0.0,active:false,released:false}; 10];
    let mut delay = [0f32; 44100];
    let mut delay_n = 0;
    
    
    let mut reverbs = [ReverbBuffer::new(1597),ReverbBuffer::new(2083),ReverbBuffer::new(2729),
    ReverbBuffer::new(3259),ReverbBuffer::new(4177),ReverbBuffer::new(5279),ReverbBuffer::new(6737),
    ReverbBuffer::new(7883),ReverbBuffer::new(9173),ReverbBuffer::new(10151),ReverbBuffer::new(11497),
    ReverbBuffer::new(13367),ReverbBuffer::new(14029)];
    
    
    let mut next_value = move|| {
        sample_clock = (sample_clock + 1.0) % sample_rate;
        //sample_clock = (sample_clock + 1.0);
        
        while let Ok(e) = rx.try_recv() {
            if (0x90^e.message.status <= 0xf) { // Midi note on            
                let mut voice: Voice = Voice{freq:0.0,midi_note:0,amplitude:1.0,time_attack:0.0,time_decay:0.0,time:0.0,active:false,released:false};
                let freq: f32 = (440. / 32.) * 2f32.powf((e.message.data1 as f32 - 9.0) / 12.);
                voice.freq = freq as i32 as f32; //Fixes clicking if sample clock is set to reset every second
                voice.active = true;
                voice.midi_note = e.message.data1;
                voice.amplitude = 0.6; //Fix this---------------
                
                
                let mut voice_number = 0;
                let mut replace = true;
                for n in 0..10 {
                    if(voices[n].released) {
                        voice_number = n;
                        replace = false;
                        break;
                    }
                }
                if(replace) {
                    voice_number = voice_n;
                }
                
                voices[voice_number] = voice;
                
                voice_n = voice_n + 1;
                if (voice_n > 9) {
                    voice_n = 0;
                }
            } else if (0x80^e.message.status <= 0xf) { // Midi note off
                for n in 0..10 {
                    if(voices[n].midi_note == e.message.data1) {
                        voices[n].released = true;
                    }
               }
            }
        }

        let mut output: f32 = 0.0;
        for n in 0..10 {
            if(voices[n].active) {
                if(voices[n].time_attack < 0.1*sample_rate && !voices[n].released) {
                    voices[n].amplitude = (20.0*voices[n].time_attack)/sample_rate;
                    voices[n].time_attack = voices[n].time_attack + 1.0;
                }            
                else {
                    if(voices[n].released) {
                        voices[n].amplitude = voices[n].amplitude - 0.0005f32;
                    }
                    voices[n].amplitude = voices[n].amplitude - 0.000001f32;
                    if(voices[n].amplitude <= 0f32) {
                        voices[n].active = false;
                    } 
                }
                
                let mut phase: f32 = (sample_clock/sample_rate * (voices[n].freq * 2.0 * 3.141592));
                
                match instrument {

                
                //phase = phase *(1.0+0.5*((4.0*(sample_clock/sample_rate))*3.141592).sin());
                //phase = phase + voices[n].freq*0.001*(4.0*2.0*3.141592*sample_clock/sample_rate).sin()
                //+ 0.01*(2.0*2.0*3.141592*sample_clock/sample_rate).sin();
                
                Instrument::Orgel => {
                // Orgel lyd
                
                let out: f32 = 0.1 * voices[n].amplitude*phase.sin()
                + 0.1 * voices[n].amplitude*(phase * 2.0).sin()
                + 0.1 * voices[n].amplitude*(phase * 3.0).sin()
                + 0.3 * voices[n].amplitude*(phase * 4.0).sin()
                + 0.2 * voices[n].amplitude*(phase * 6.0).sin()
                + 0.2 * voices[n].amplitude*(phase * 8.0).sin()
                + 0.1 * voices[n].amplitude*(phase * 10.0).sin();

                output = output + out * 0.1;

                },

                Instrument::Weird => {
                let t: f32 = phase;

                // Skørbølge med 25 Fourierled
                let out: f32 = 0.5*voices[n].amplitude*(-0.007918*(15.708*t).cos()
                +0.038073*(15.708*t).sin()
                +0.022659*(15.0796*t).cos()
                +0.019358*(15.0796*t).sin()
                +0.000969*(14.4513*t).cos()
                -0.009212*(14.4513*t).sin()
                -0.019904*(13.823*t).cos()
                +0.004311*(13.823*t).sin()
                -0.026721*(13.1947*t).cos()
                +0.018882*(13.1947*t).sin()
                -0.011172*(12.5664*t).cos()
                +0.047797*(12.5664*t).sin()
                +0.029089*(11.9381*t).cos()
                +0.023723*(11.9381*t).sin()
                -0.001463*(11.3097*t).cos()
                -0.01358*(11.3097*t).sin()
                -0.027469*(10.6814*t).cos()
                +0.007344*(10.6814*t).sin()
                -0.035268*(10.0531*t).cos()
                +0.023627*(10.0531*t).sin()
                -0.017356*(9.42478*t).cos()
                +0.064208*(9.42478*t).sin()
                +0.04064*(8.79646*t).cos()
                +0.03071*(8.79646*t).sin()
                -0.008001*(8.16814*t).cos()
                -0.022867*(8.16814*t).sin()
                -0.043*(7.53982*t).cos()
                +0.014937*(7.53982*t).sin()
                -0.051253*(6.9115*t).cos()
                +0.031978*(6.9115*t).sin()
                -0.032015*(6.28319*t).cos()
                +0.097756*(6.28319*t).sin()
                +0.067429*(5.65487*t).cos()
                +0.043588*(5.65487*t).sin()
                -0.031871*(5.02655*t).cos()
                -0.050568*(5.02655*t).sin()
                -0.088597*(4.39823*t).cos()
                +0.043866*(4.39823*t).sin()
                -0.089768*(3.76991*t).cos()
                +0.051323*(3.76991*t).sin()
                -0.088465*(3.14159*t).cos()
                +0.203019*(3.14159*t).sin()
                +0.189877*(2.51327*t).cos()
                +0.069533*(2.51327*t).sin()
                -0.253464*(1.88496*t).cos()
                -0.271843*(1.88496*t).sin()
                -0.546645*(1.25664*t).cos()
                +0.49343*(1.25664*t).sin()
                +0.15225*(0.628319*t).cos()
                +0.12749*(0.628319*t).sin()
                -0.255069);
                
                output = output + out * 0.1;
                },

                Instrument::SoftSaw => {
                let t: f32 = phase;

                // Sav
                let out: f32 = 0.5*voices[n].amplitude*(
                -(40.0*t).sin()/20.0
                +(38.0*t).sin()/19.0
                -(36.0*t).sin()/18.0
                +(34.0*t).sin()/17.0
                -(32.0*t).sin()/16.0
                +(30.0*t).sin()/15.0
                -(28.0*t).sin()/14.0
                +(26.0*t).sin()/13.0
                -(24.0*t).sin()/12.0
                +(22.0*t).sin()/11.0
                -(20.0*t).sin()/10.0
                +(18.0*t).sin()/9.0
                -(16.0*t).sin()/8.0
                +(14.0*t).sin()/7.0
                -(12.0*t).sin()/6.0
                +(10.0*t).sin()/5.0
                -(8.0*t).sin()/4.0
                +(6.0*t).sin()/3.0
                -(4.0*t).sin()/2.0);

                
                output = output + out * 0.1;
                },

                Instrument::HardSaw => {
                let t: f32 = phase;

                // Skørbølge med 25 Fourierled
                let out: f32 = 0.5*voices[n].amplitude*(t%1.0);

                
                output = output + out * 0.1;
                },
                
                Instrument::Brass => {
                let t: f32 = phase;
                let out: f32 = 0.5*voices[n].amplitude*(
                    1.17*(1.0*t).sin()
                    +2.33*(2.0*t).sin()
                    +1.4*(3.0*t).sin()
                    +0.85*(4.0*t).sin()
                    +0.28*(5.0*t).sin()
                    +0.11*(6.0*t).sin()
                    +0.05*(7.0*t).sin()
                    +0.02*(8.0*t).sin()
                    +0.008*(9.0*t).sin()
                    +0.003*(10.0*t).sin()

                );
                output = output + out * 0.1;

                }
                /*1 1.17
                2 2.33
                3 1.40
                4 0.85
                5 0.28
                6 0.11
                7 0.05
                8 0.02
                9 0.008
                10 0.003*/
                }
                //output = output + out * 0.1;
                
            }
        }
        
        output = output + 0.04 * delay[delay_n];
        delay[delay_n] = output;
        
        delay_n = delay_n + 1;
        if(delay_n >= 23009) {
            delay_n = 0;
        }

        
        let mut x: f32 = 0.05 * output;
        for n in 0..reverbs.len()  {
            x = x + 0.05 * reverbs[n].buffer[reverbs[n].pos];
        }
        
        for n in 0..reverbs.len()  {
            reverbs[n].buffer[reverbs[n].pos] = x;
        }
        
        for n in 0..reverbs.len() {
            reverbs[n].increment();
        }
        
        output = output*0.9 + 4.0 * x;
        
        if(output >= 1. || output <= -1.) {
            println!("Output too high! {}", output);
        }
        //output =  0.4 *(sample_clock * (440.0 * 2.0 * 3.141592) / sample_rate).sin();
        //println!("amplitude: {}", output);
        //println!("{}", sample_clock);
        
        //For analysis
        if(false && (sample_clock % 20.0) == 0.0) {
            println!("{}", output);
        }
        output
    };
    

    let input_thread = thread::spawn(move|| {
        use std::io::{stdin,stdout,Write};
        let mut s=String::new();
        print!("Please enter some text: ");
        let _=stdout().flush();
        stdin().read_line(&mut s).expect("Did not enter a correct string");
        if let Some('\n')=s.chars().next_back() {
            s.pop();
        }
        if let Some('\r')=s.chars().next_back() {
            s.pop();
        }
        println!("You typed: {}",s);
     
    });
    
    event_loop.run(move |id, result| {
        let data = match result {
            Ok(data) => data,
            Err(err) => {
                eprintln!("an error occurred on stream {:?}: {}", id, err);
                return;
            }
        };

        match data {
            cpal::StreamData::Output { buffer: cpal::UnknownTypeOutputBuffer::U16(mut buffer) } => {
                for sample in buffer.chunks_mut(format.channels as usize) {
                    let value = ((next_value() * 0.5 + 0.5) * std::u16::MAX as f32) as u16;
                    for out in sample.iter_mut() {
                        *out = value;
                    }
                }
            },
            cpal::StreamData::Output { buffer: cpal::UnknownTypeOutputBuffer::I16(mut buffer) } => {
                for sample in buffer.chunks_mut(format.channels as usize) {
                    let value = (next_value() * std::i16::MAX as f32) as i16;
                    for out in sample.iter_mut() {
                        *out = value;
                    }
                }
            },
            cpal::StreamData::Output { buffer: cpal::UnknownTypeOutputBuffer::F32(mut buffer) } => {
                for sample in buffer.chunks_mut(format.channels as usize) {
                    let value = next_value();
                    for out in sample.iter_mut() {
                        *out = value;
                    }
                }
            },
            _ => (),
        }
    });

}

