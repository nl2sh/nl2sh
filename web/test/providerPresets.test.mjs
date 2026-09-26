import assert from 'node:assert/strict';
import test from 'node:test';
import {beginnerProviders,initialBeginnerProvider} from '../src/providerPresets.ts';

test('quick start prefers DeepSeek and includes all existing provider presets',()=>{
  assert.equal(beginnerProviders[0].id,'deepseek');
  assert.equal(beginnerProviders[0].model,'deepseek-flash');
  assert.deepEqual(beginnerProviders.map(item=>item.id),[
    'deepseek','openrouter','openai','moonshot','siliconflow','ollama','custom',
  ]);
  assert.equal(beginnerProviders[2].name,'OpenAI');
  assert.equal(beginnerProviders[2].editableEndpoint,false);
  assert.equal(beginnerProviders[2].needsKey,true);
  assert.equal(beginnerProviders[6].editableEndpoint,true);
  assert.equal(beginnerProviders[6].needsKey,false);
});

test('unconfigured default selects DeepSeek while existing settings are preserved',()=>{
  assert.deepEqual(initialBeginnerProvider({endpoint:'https://openrouter.ai/api/v1',api_key:'',model:'openrouter/free'}),{
    index:0,endpoint:'https://api.deepseek.com',model:'deepseek-flash',key:'',
  });
  assert.deepEqual(initialBeginnerProvider({endpoint:'https://api.deepseek.com',api_key:'configured-key',model:'selected-model'}),{
    index:0,endpoint:'https://api.deepseek.com',model:'selected-model',key:'configured-key',
  });
  assert.equal(initialBeginnerProvider({endpoint:'https://api.deepseek.com',api_key:'',model:'openrouter/free'}).model,'deepseek-flash');
  assert.equal(initialBeginnerProvider({endpoint:'https://example.com/v1',model:'local-model'}).index,6);
});
