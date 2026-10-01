import assert from 'node:assert/strict';
import test from 'node:test';
import {decodePcm,parseWavHeader} from '../src/audioPcm.ts';

test('WAV header fills playback parameters and PCM samples decode by channel',()=>{
  const bytes=new ArrayBuffer(48),view=new DataView(bytes);
  const write=(offset,text)=>{for(let i=0;i<text.length;i++)view.setUint8(offset+i,text.charCodeAt(i))};
  write(0,'RIFF');view.setUint32(4,40,true);write(8,'WAVE');write(12,'fmt ');
  view.setUint32(16,16,true);view.setUint16(20,1,true);view.setUint16(22,2,true);
  view.setUint32(24,48000,true);view.setUint32(28,192000,true);
  view.setUint16(32,4,true);view.setUint16(34,16,true);write(36,'data');view.setUint32(40,4,true);
  view.setInt16(44,16384,true);view.setInt16(46,-16384,true);
  const header=parseWavHeader(bytes);
  assert.deepEqual(header,{sampleRate:48000,channels:2,format:'s16le',dataOffset:44,dataLength:4});
  const samples=decodePcm(bytes,header,header.dataOffset,header.dataLength);
  assert.equal(samples[0][0],0.5);assert.equal(samples[1][0],-0.5);
});

test('raw PCM supports unsigned, 24 bit and float samples',()=>{
  const u8=decodePcm(Uint8Array.from([0,128,255]).buffer,{sampleRate:8000,channels:1,format:'u8'});
  assert.equal(u8[0][0],-1);assert.equal(u8[0][1],0);assert.ok(u8[0][2]>0.99);
  const s24=decodePcm(Uint8Array.from([0,0,0x40,0,0,0xc0]).buffer,{sampleRate:44100,channels:1,format:'s24le'});
  assert.equal(s24[0][0],0.5);assert.equal(s24[0][1],-0.5);
  const float=new Float32Array([0.25,-0.25]);
  assert.deepEqual([...decodePcm(float.buffer,{sampleRate:44100,channels:1,format:'f32le'})[0]],[0.25,-0.25]);
});

test('invalid WAV and PCM settings fail without guessing',()=>{
  assert.throws(()=>parseWavHeader(new ArrayBuffer(8)),/RIFF/);
  assert.throws(()=>decodePcm(new ArrayBuffer(4),{sampleRate:0,channels:1,format:'s16le'}),/采样率/);
  assert.throws(()=>decodePcm(new ArrayBuffer(4),{sampleRate:44100,channels:0,format:'s16le'}),/声道/);
});
