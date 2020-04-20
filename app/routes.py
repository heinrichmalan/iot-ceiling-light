from app import app
from flask import request, jsonify
import redis

MAX_BRIGHTNESS = 1000000

@app.route('/')
@app.route('/index')
def index():
    return 'Hello, World!'

def get_redis():
    r = redis.Redis(host="localhost", port=6379, db=0, decode_responses=True)
    return r

def get_cache_value(key):
    r = get_redis()
    return r.get(key)

def set_cache_value(key, value):
    r = get_redis()
    return r.set(key, value)

@app.route('/api/lightStatus')
def light_status():
    return jsonify({ "currentState": get_cache_value('status') })

@app.route('/api/lightOn')
def light_on():
    status = "1"
    set_cache_value('status', status)
    return jsonify({ "currentState": status })

@app.route('/api/lightOff')
def light_off():
    status = "0"
    set_cache_value('status', status)
    return jsonify({ "currentState": status })

@app.route('/api/getBrightness')
def getBrightness():
    brightness = int(get_cache_value('brightness'))
    centigrade = int((brightness/MAX_BRIGHTNESS)*100)
    return jsonify({ "currentState": centigrade })

@app.route('/api/setBrightness')
def setBrightness():
    brightness_set = request.args.get('brightness', default="100", type=int)
    
    brightness = int((int(brightness_set)/100) * MAX_BRIGHTNESS)

    set_cache_value('brightness', brightness)

    return jsonify({ "currentState": brightness_set })


