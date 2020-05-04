
use std::{env, thread, time, cmp};

use chrono::prelude::*;
use rust_pigpio;
use rumqtt::{MqttClient, MqttOptions, QoS, Receiver};
use rumqtt::mqttoptions::SecurityOptions;
use rumqtt::client::Notification;


const PWM_PIN: u32= 18;
const PWM_FREQ: u32= 2000;
const MAX_BRIGHTNESS: u32= 1000000;
const MIN_BRIGHTNESS: u32= 0;
const SLEEP_TIME: f64 = 0.05;
static STATUS_ON: &str = "1";
static STATUS_OFF: &str = "0";

struct Bulb {
    current_brightness: u32,
    target_brightness: u32,
    current_status: String
}

impl Bulb {
    fn new() -> Self {
        Self {
            current_brightness: 0,
            target_brightness: 0,
            current_status: String::from("0")
        }
    }

    fn set_bulb_brightness(&mut self, target_brightness: u32) {
        let current_brightness = self.current_brightness;
        let pct_increase_allowed = SLEEP_TIME / 2.0;
        let brightness_increase_allowed = (pct_increase_allowed * (MAX_BRIGHTNESS as f64)) as u32;
        let mut brightness = target_brightness;
        if target_brightness > current_brightness {
            brightness = cmp::min(current_brightness + brightness_increase_allowed, target_brightness);
        } else if target_brightness < current_brightness {
            if brightness_increase_allowed > current_brightness {
                brightness = 0;
            } else {
                brightness = cmp::max(current_brightness - brightness_increase_allowed, target_brightness);
            }
        }

        if brightness < MIN_BRIGHTNESS || brightness > MAX_BRIGHTNESS {
            println!("Brightness outside limits: {}. Constraining to range {} - {}", brightness, MIN_BRIGHTNESS, MAX_BRIGHTNESS);
            brightness = cmp::max(cmp::min(brightness, MAX_BRIGHTNESS), MIN_BRIGHTNESS);
        }

        println!("Brightness: {}", brightness);
        rust_pigpio::pwm::hardware_pwm(PWM_PIN, PWM_FREQ, brightness).expect("Should be able to write to GPIO pins");

        self.current_brightness = brightness;
    }
    
    fn turn_off_bulb(&mut self) {
        self.set_bulb_brightness(0);
    }
}

struct Schedule {
    hour: u32,
    minute: u32,
    duration: u32,
    running: bool
}

struct MqttHandler {
    mqtt_client: MqttClient,
    notifications: Receiver<Notification>
}

impl MqttHandler {
    fn new() -> Self {
        let mut mqtt_options = MqttOptions::new("test-pubsub1", "192.168.0.240", 1883);
        let sec_opts = SecurityOptions::UsernamePassword(String::from("mqttuser"), String::from("Kawaiinekodesuy0"));
        mqtt_options = mqtt_options.set_security_opts(sec_opts);
        let (mut mqtt_client, notifications) = MqttClient::start(mqtt_options).unwrap();
        mqtt_client.subscribe("bedroom/light/#", QoS::AtLeastOnce).unwrap();
        Self {
            mqtt_client,
            notifications
        }
    }

    fn check_notifications(&mut self, bulb: &mut Bulb) {
        let mut msg = self.notifications.try_recv();

        while msg.is_ok() {
            let n = msg.unwrap();
            match n {
                Notification::Publish(pl) => {
                    let topic: String = String::from(&pl.topic_name);
                    let message: String = String::from(std::str::from_utf8(&pl.payload).unwrap());
                    println!("topic: {:?} - msg: {:?}", &topic, &message);
                    match topic.as_str() {
                        "bedroom/light/switch" => {
                            bulb.current_status = message;
                            self.set_status(String::from(&bulb.current_status).as_str());
                        },
                        "bedroom/light/brightness/set" => {
                            bulb.target_brightness = message.parse().expect("Should be an integer");
                            self.set_brightness(bulb.target_brightness);
                        },
                        _ => {}
                    }
                },
                _ => {}
            }
            msg = self.notifications.try_recv();
        }
    }
    
    fn set_status(&mut self, status: &str) {
        self.mqtt_client.publish("bedroom/light/status", QoS::ExactlyOnce, false, status).unwrap();
    }
    
    fn set_brightness(&mut self, brightness: u32) {
        self.mqtt_client.publish("bedroom/light/brightness", QoS::ExactlyOnce, false, brightness.to_string().as_bytes()).unwrap();
    }
}

impl Schedule {
    fn new(hour: u32, minute: u32, duration: u32) -> Self {
        Self {
            hour,
            minute,
            duration,
            running: false
        }
    }

    fn calc_brightness(&self) -> u32 {
        let current_time : DateTime<Local> = Local::now();
        let start_time_seconds = self.hour*3600 + self.minute*60;
        let curr_time_seconds = current_time.hour()*3600 + current_time.minute()*60 + current_time.second();

        let pct_progress = curr_time_seconds as f64 - start_time_seconds as f64 / (self.duration * 60) as f64;
        let brightness = (MAX_BRIGHTNESS as f64 * pct_progress) as u32;
        brightness
    }

    fn should_run(&self) -> bool {
        let current_time : DateTime<Local> = Local::now();
        let start_time_seconds = self.hour*3600 + self.minute*60;
        let curr_time_seconds = current_time.hour()*3600 + current_time.minute()*60 + current_time.second();
        let end_time_seconds = start_time_seconds + self.duration*60;

        curr_time_seconds >= start_time_seconds && curr_time_seconds <= end_time_seconds
    }

}


fn main() {
    let args: Vec<String> = env::args().collect();
    let mut redis_host = String::from("redis://127.0.0.1/");

    for arg in &args {
        let parts: Vec<&str> = arg.split("=").collect();
        if parts.len() != 2 {
            continue;
        }
        if parts[0] == "--redis-host"{
            redis_host = format!("redis://{}/", parts[1]);
        }
    }
    println!("Initializing GPIO");
    rust_pigpio::initialize().expect("Should be able to initialize GPIO");
    let mut bulb = Bulb::new();

    println!("Connecting to mqtt at {}", redis_host);
    let mut mqtt_handler = MqttHandler::new();
    
    let mut schedule = Schedule::new(6, 30, 30);

    loop {
        mqtt_handler.check_notifications(&mut bulb);

        if bulb.current_status == "0" && bulb.current_brightness != 0 {
            bulb.turn_off_bulb();
        }

        if schedule.should_run() {
            if bulb.current_status == "0" && !schedule.running {
                println!("Starting to run Schedule");
                schedule.running = true;
                bulb.current_status = String::from("1");
                mqtt_handler.set_status(STATUS_ON);
                mqtt_handler.set_brightness(0);
                bulb.turn_off_bulb();
            } else if bulb.current_status == "0" && schedule.running {
                println!("Schedule cancelled by turning off light during schedule");
                bulb.current_status = String::from("0");
                mqtt_handler.set_status(STATUS_OFF);
                bulb.turn_off_bulb();
                schedule.running = false;
            }

            if bulb.current_status == "1" && schedule.running {
                bulb.target_brightness = cmp::min(MAX_BRIGHTNESS, schedule.calc_brightness());
                mqtt_handler.set_brightness(bulb.target_brightness);
            }
        } else if schedule.running {
            schedule.running = false;
        }

        mqtt_handler.check_notifications(&mut bulb);

        if bulb.current_brightness != bulb.target_brightness {
            if bulb.current_status != "0"{
                bulb.set_bulb_brightness(bulb.target_brightness);
            } else if bulb.current_status == "0" && bulb.current_brightness != 0 {
                bulb.set_bulb_brightness(0);
            }
        }
        thread::sleep(time::Duration::from_millis((SLEEP_TIME * 1000 as f64) as u64));
    }
}
