#!/usr/bin/python3.7
import datetime
import redis
import pigpio
import time

PWM_PIN = 18
PWM_FREQ = 2000
MAX_BRIGHTNESS = 1000000
MIN_BRIGHTNESS = 0
SLEEP_TIME = 0.05

r = redis.Redis(host="localhost", port=6379, db=0, decode_responses=True)

if not r.get('status'):
    r.set('status', '1')

if not r.get('brightness'):
    r.set('brightness', MAX_BRIGHTNESS)

pi = pigpio.pi()

class Bulb:
    current_brightness = 0
    current_status = "0"

    def __init__(self, current_status):
        self.current_status = current_status

    def set_bulb_brightness(self, target_brightness: int):
        current_brightness = self.current_brightness
        pct_increase_allowed = (SLEEP_TIME/2)
        brightness_increase_allowed = int(pct_increase_allowed * MAX_BRIGHTNESS)
        brightness = target_brightness
        if target_brightness > current_brightness:
            brightness = min((current_brightness + brightness_increase_allowed), target_brightness)
        else:
            brightness = max((current_brightness - brightness_increase_allowed), target_brightness)
    
        if brightness < MIN_BRIGHTNESS or brightness > MAX_BRIGHTNESS:
            print(f"Brightness outside limits: {brightness}. Contraining to range {MIN_BRIGHTNESS} - {MAX_BRIGHTNESS}")
            brightness = max(min(brightness, MAX_BRIGHTNESS), MIN_BRIGHTNESS)
        print(f"Brightness: {brightness}")
        pi.hardware_PWM(PWM_PIN, PWM_FREQ, brightness)
    
        self.current_brightness = brightness
    
    def turn_off_bulb(self):
        self.set_bulb_brightness(0)
    

def calc_sched_brightness(sched):
    duration = sched['duration']
    curr_time = datetime.datetime.now()
    start_time_seconds = sched['hour']*3600 + sched['minute']*60
    curr_time_seconds = curr_time.hour*3600 + curr_time.minute*60+curr_time.second
 
    pct_progress = ((curr_time_seconds - start_time_seconds) / (duration * 60) ) 
    assert pct_progress >= 0.0 and pct_progress <= 1.0, "Percentage is wrong"
    brightness = int(MAX_BRIGHTNESS * pct_progress)
    #print(f"pct_progress: {pct_progress}")
    return brightness 

def run_schedule(sched):
    curr_time = datetime.datetime.now()
    end_time = curr_time + datetime.timedelta(minutes=sched['duration'])
    start_time_seconds = sched['hour']*3600 + sched['minute']*60
    curr_time_seconds = curr_time.hour*3600 + curr_time.minute*60+curr_time.second
    end_time_seconds = start_time_seconds + sched['duration']*60 
    #print(f"Start: {start_time_seconds}, Curr: {curr_time_seconds}, end: {end_time_seconds}")
    return  (curr_time_seconds >= start_time_seconds and curr_time_seconds <= end_time_seconds)

if __name__ == "__main__":
    target_brightness = int(r.get('brightness'))
    current_status = r.get('status')
    bulb = Bulb(current_status=current_status)
    #print(f"Starting Brightness: {current_brightness}")

    # maybe hash the schedules to know when to update the redis queue
    schedule = { "hour": 7, "minute": 0, "duration": 30 }
    running_schedule = False
    while True:
        print(f"Status: {bulb.current_status}, Brightness: {bulb.current_brightness}")
        status = r.get('status')
        if status != bulb.current_status:
            bulb.current_status = status
            if bulb.current_status == "0":
                #bulb.current_brightness = 0
                bulb.turn_off_bulb() 
            else:
                bulb.set_bulb_brightness(target_brightness) 
        
        if run_schedule(schedule):
            print("Run sched true")
            if bulb.current_status == "0" and not running_schedule:
                running_schedule = True
                bulb.current_status = "1"
                r.set("status", "1")
                r.set("brightness", "0")
                bulb.turn_off_bulb()
            elif current_status == "0" and running_schedule:
                bulb.current_status = "0"
                r.set('status', '0')
                bulb.turn_off_bulb()
                running_schedule = False
    
            if bulb.current_status == "1" and running_schedule:
                r.set("brightness", min(MAX_BRIGHTNESS ,int(calc_sched_brightness(schedule))))
        elif running_schedule:
            running_schedule = False
    
    
        
        target_brightness = int(r.get('brightness'))
        #print(f"final brightness: {brightness}")
        if bulb.current_brightness != target_brightness:
            if bulb.current_status != "0":
                bulb.set_bulb_brightness(target_brightness)

            elif (bulb.current_status == "0" and bulb.current_brightness != 0):
                bulb.set_bulb_brightness(0)
    
        time.sleep(SLEEP_TIME)
