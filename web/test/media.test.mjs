import assert from 'node:assert/strict';
import test from 'node:test';
import {localMedia} from '../src/media.ts';
test('local media resolves supported paths and encoded preview URLs',()=>{
  assert.deepEqual(localMedia('/sdcard/a%20b.mp4'),{path:'/sdcard/a b.mp4',kind:'video'});
  assert.deepEqual(localMedia('/api/file-preview?path=%2Fsdcard%2F100%25.png'),{path:'/sdcard/100%.png',kind:'image'});
  assert.equal(localMedia('~/a.wav')?.kind,'audio');
  for(const path of ['javascript:foo.png','file:///a.png','//example.com/a.png','https://example.com/a.png','/a.svg','/a.txt','/a%00.png','/a%zz.png'])assert.equal(localMedia(path),null);
});
