use chrono::prelude::*;
use redis;
use redis::Commands;
use rust_pigpio;

use std::{thread, time, cmp};

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
            brightness = cmp::min((current_brightness + brightness_increase_allowed), target_brightness);
        } else if target_brightness < current_brightness {
            brightness = cmp::max((current_brightness - brightness_increase_allowed), target_brightness);
        }

        if brightness < MIN_BRIGHTNESS || brightness > MAX_BRIGHTNESS {
            println!("Brightness outside limits: {}. Constraining to range {} - {}", brightness, MIN_BRIGHTNESS, MAX_BRIGHTNESS);
            brightness = cmp::max(cmp::min(brightness, MAX_BRIGHTNESS), MIN_BRIGHTNESS);
        }

        println!("Brightness: {}", brightness);
        rust_pigpio::pwm::hardware_pwm(PWM_PIN, PWM_FREQ, brightness);

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

        let pct_progress = ((curr_time_seconds - start_time_seconds) as f64 / (self.duration * 60) as f64);
        let brightness = (MAX_BRIGHTNESS as f64 * pct_progress) as u32;
        brightness
    }

    fn should_run(&self) -> bool {
        let current_time : DateTime<Local> = Local::now();
        let start_time_seconds = self.hour*3600 + self.minute*60;
        let curr_time_seconds = current_time.hour()*3600 + current_time.minute()*60 + current_time.second();
        let end_time_seconds = start_time_seconds + self.duration*60;

        (curr_time_seconds >= start_time_seconds && curr_time_seconds <= end_time_seconds)
    }

}

fn get_status(con: &mut redis::Connection) -> String {
    let status: String = con.get("status").expect("Status should have been set");
    status
}

fn set_status(con: &mut redis::Connection, status: &str) {
    let _ : () = con.set("status", status).expect("Failed to set status");
}

fn get_brightness(con: &mut redis::Connection) -> u32 {
    let brightness: u32 = con.get("brightness").expect("Brightness should have been set");
    brightness
}

fn set_brightness(con: &mut redis::Connection, brightness: u32) {
    let _ : () = con.set("brightness", brightness).expect("Failed to set brightness");
}


fn main() {
    rust_pigpio::initialize();
    let mut bulb = Bulb::new();
    let client = redis::Client::open("redis://127.0.0.1/").expect("Could not open client");
    let mut con = client.get_connection().expect("Could not get connection");

    let mut target_brightness: u32 = con.get("brightness").unwrap_or(MAX_BRIGHTNESS);
    set_brightness(&mut con, target_brightness);
    
    let mut status: String = con.get("status").unwrap_or(String::from("1")); 
    set_status(&mut con, &status);
    bulb.current_status = String::clone(&status);
    
    let mut schedule = Schedule::new(6, 30, 30);

    loop {
        // println!("Status: {}, Brightness: {}", &status, target_brightness);
        status = get_status(&mut con);

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
                set_status(&mut con, STATUS_ON);
                set_brightness(&mut con, 0);
                bulb.turn_off_bulb();
            } else if status == "0" && schedule.running {
                bulb.current_status = String::from("0");
                set_status(&mut con, STATUS_OFF);
                bulb.turn_off_bulb();
                schedule.running = false;
            }

            if bulb.current_status == "1" && schedule.running {
                set_brightness(&mut con, cmp::min(MAX_BRIGHTNESS, schedule.calc_brightness()));
            }
        } else if schedule.running {
            schedule.running = false;
        }

        target_brightness = get_brightness(&mut con);

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
