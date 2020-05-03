
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
    current_status: String
}

impl Bulb {
    fn new() -> Self {
        Self {
            current_brightness: 0,
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
            brightness = cmp::max(current_brightness - brightness_increase_allowed, target_brightness);
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
        let (mqtt_client, notifications) = MqttClient::start(mqtt_options).unwrap();
        mqtt_client.subscribe("bedroom/light/#", QoS::AtLeastOnce).unwrap();
        Self {
            mqtt_client,
            notifications
        }
    }

    fn check_notifications_for_topic(&self, to_match: &str) -> Option<String> {
        let msg = self.notifications.try_recv();

        match msg {
            Ok(n) => {
                match n {
                    Notification::Publish(pl) => {
                        let topic: String = String::from(&pl.topic_name);
                        let message: String = String::from(std::str::from_utf8(&pl.payload).unwrap());
                        println!("topic: {:?} - msg: {:?}", &topic, &message);
                        println!("To match: {}", to_match);
                        if topic.as_str() == to_match {
                            return Some(message);
                        } else {
                            return None
                        }
                    },
                    _ => {
                        return None
                    }
                }
            },
            _ => {
                return None
            }
        }
    }

    
    fn get_status(&self) -> Option<String> {
        self.check_notifications_for_topic("bedroom/light/switch")
    }
    
    fn set_status(&mut self, status: &str) {
        self.mqtt_client.publish("bedroom/light/status", QoS::ExactlyOnce, false, status).unwrap();
    }
    
    fn get_brightness(&self) -> Option<String> {
        self.check_notifications_for_topic("bedroom/light/brightness/set")
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
    
    println!("Checking for data in mqtt");
    let target_brightness: String = mqtt_handler.get_brightness().unwrap_or(MAX_BRIGHTNESS.to_string());
    let mut target_brightness: u32 = target_brightness.parse().expect("Should be an integer");
    mqtt_handler.set_brightness(target_brightness);
    
    let mut status: String = mqtt_handler.get_brightness().unwrap_or(String::from("1"));
    mqtt_handler.set_status(&status);
    bulb.current_status = String::clone(&status);
    
    let mut schedule = Schedule::new(6, 30, 30);

    loop {
        // println!("Status: {}, Brightness: {}", &status, target_brightness);
        status = mqtt_handler.get_status().unwrap_or(String::from("0"));

        if status != bulb.current_status {
            bulb.current_status = String::clone(&status);
            if bulb.current_status == "0" {
                bulb.turn_off_bulb();
            } else {
                bulb.set_bulb_brightness(target_brightness);
            }
        }

        if schedule.should_run() {
            println!("Run sched true");
            if bulb.current_status == "0" && !schedule.running {
                schedule.running = true;
                bulb.current_status = String::from("1");
                mqtt_handler.set_status(STATUS_ON);
                mqtt_handler.set_brightness(0);
                bulb.turn_off_bulb();
            } else if status == "0" && schedule.running {
                bulb.current_status = String::from("0");
                mqtt_handler.set_status(STATUS_OFF);
                bulb.turn_off_bulb();
                schedule.running = false;
            }

            if bulb.current_status == "1" && schedule.running {
                mqtt_handler.set_brightness(cmp::min(MAX_BRIGHTNESS, schedule.calc_brightness()));
            }
        } else if schedule.running {
            schedule.running = false;
        }

        target_brightness = mqtt_handler.get_brightness().unwrap_or(String::from("0")).parse().expect("Should be an integer");

        if bulb.current_brightness != target_brightness {
            if bulb.current_status != "0"{
                bulb.set_bulb_brightness(target_brightness);
            } else if bulb.current_status == "0" && bulb.current_brightness != 0 {
                bulb.set_bulb_brightness(0);
            }
        }
        thread::sleep(time::Duration::from_millis((SLEEP_TIME * 1000 as f64) as u64));
    }
}
