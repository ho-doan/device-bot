const {test}=require('node:test'); const assert=require('node:assert/strict');
const vm=require('node:vm');const fs=require('node:fs');
const script=fs.readFileSync(__dirname+'/../src/live.js','utf8').replace('__KEY__','"capture"').replace('__OFFER__','"offer"');
function fixture(denied=false){
 const events=[];let stopped=false;
 const track={stop(){stopped=true},async cropTo(target){events.push('crop')}};
 const context={navigator:{mediaDevices:{async getDisplayMedia(){if(denied)throw Error('denied');events.push('capture');return {getVideoTracks:()=>[track],getTracks:()=>[track]}}}},
 CropTarget:{fromElement:async()=>({})},document:{documentElement:{}},innerWidth:800,innerHeight:600,
 setTimeout:()=>1,clearTimeout(){},addEventListener(){},removeEventListener(){},
 RTCPeerConnection:class{constructor(){this.iceGatheringState='complete';this.connectionState='new'} addTrack(){events.push('track')} async setRemoteDescription(o){assert.equal(o.sdp,'offer')} async createAnswer(){return {type:'answer',sdp:'answer'}}async setLocalDescription(a){this.localDescription=a}close(){this.connectionState='closed'}}};
 context.window=context;vm.runInNewContext(script,context);return {context,events,stopped:()=>stopped};
}
test('crop to requested document before adding video; answer and cleanup',async()=>{
 const f=fixture();await new Promise(setImmediate);
 assert.equal(f.context.capture.answer.sdp,'answer'); assert.equal(f.context.capture.answer.width,800);
 assert.deepEqual(f.events,['capture','crop','track']);f.context.capture.stop();assert.ok(f.stopped());
});
test('sharing rejection is surfaced and never adds a track',async()=>{const f=fixture(true);await new Promise(setImmediate);assert.match(f.context.capture.error,/denied/);assert.deepEqual(f.events,[])});

test('closing while sharing prompt is pending stops late tracks',async()=>{const f=fixture();f.context.capture.stop();await new Promise(setImmediate);assert.ok(f.stopped());assert.deepEqual(f.events,['capture']);assert.equal(f.context.capture.answer,null)});
