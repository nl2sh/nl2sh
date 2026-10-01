export type PcmFormat='u8'|'s16le'|'s24le'|'s32le'|'f32le';
export type PcmSettings={sampleRate:number;channels:number;format:PcmFormat};
export type WavInfo=PcmSettings&{dataOffset:number;dataLength:number};

const sampleBytes:Record<PcmFormat,number>={u8:1,s16le:2,s24le:3,s32le:4,f32le:4};
export const pcmFormats:{value:PcmFormat;label:string}[]=[
  {value:'u8',label:'8 位无符号 PCM'},
  {value:'s16le',label:'16 位有符号 PCM（小端）'},
  {value:'s24le',label:'24 位有符号 PCM（小端）'},
  {value:'s32le',label:'32 位有符号 PCM（小端）'},
  {value:'f32le',label:'32 位浮点 PCM（小端）'},
];

export function parseWavHeader(buffer:ArrayBuffer):WavInfo{
  const view=new DataView(buffer);
  if(view.byteLength<12||ascii(view,0,4)!=='RIFF'||ascii(view,8,4)!=='WAVE')throw new Error('不是标准 RIFF/WAVE 文件');
  let settings:PcmSettings|null=null,dataOffset=-1,dataLength=0;
  for(let offset=12;offset+8<=view.byteLength;){
    const name=ascii(view,offset,4),length=view.getUint32(offset+4,true);
    const start=offset+8;
    if(name==='fmt '&&length>=16&&start+16<=view.byteLength){
      const encoding=view.getUint16(start,true),channels=view.getUint16(start+2,true);
      const sampleRate=view.getUint32(start+4,true),bits=view.getUint16(start+14,true);
      const format=encoding===3&&bits===32?'f32le':encoding===1&&bits===8?'u8':encoding===1&&bits===16?'s16le':encoding===1&&bits===24?'s24le':encoding===1&&bits===32?'s32le':null;
      if(!format)throw new Error('此 WAV 的编码暂不支持参数播放，可尝试直接播放');
      settings={sampleRate,channels,format};
    }
    if(name==='data'){dataOffset=start;dataLength=length;break}
    const next=start+length+(length%2);
    if(next<=offset||next>view.byteLength)break;
    offset=next;
  }
  if(!settings||dataOffset<0)throw new Error('WAV 头部缺少音频格式或数据块');
  return {...settings,dataOffset,dataLength};
}

function ascii(view:DataView,offset:number,length:number){
  let text='';for(let i=0;i<length;i++)text+=String.fromCharCode(view.getUint8(offset+i));return text;
}

export function decodePcm(buffer:ArrayBuffer,settings:PcmSettings,offset=0,length=buffer.byteLength-offset):Float32Array[]{
  const {sampleRate,channels,format}=settings;
  if(!Number.isInteger(sampleRate)||sampleRate<3000||sampleRate>192000)throw new Error('采样率需在 3000–192000 Hz 之间');
  if(!Number.isInteger(channels)||channels<1||channels>8)throw new Error('声道数需在 1–8 之间');
  const bytes=sampleBytes[format];
  if(!bytes)throw new Error('不支持的 PCM 采样格式');
  if(!Number.isInteger(offset)||offset<0||!Number.isInteger(length)||length<0||offset+length>buffer.byteLength)throw new Error('音频数据范围无效');
  const frames=Math.floor(length/(channels*bytes));
  if(frames===0||frames>8_000_000)throw new Error('音频为空或过长，无法在浏览器中解码');
  const result=Array.from({length:channels},()=>new Float32Array(frames));
  const view=new DataView(buffer);
  let position=offset;
  for(let frame=0;frame<frames;frame++)for(let channel=0;channel<channels;channel++){
    let sample:number;
    switch(format){
      case 'u8':sample=(view.getUint8(position)-128)/128;break;
      case 's16le':sample=view.getInt16(position,true)/32768;break;
      case 's24le':{
        const value=view.getUint8(position)|(view.getUint8(position+1)<<8)|(view.getUint8(position+2)<<16);
        sample=((value&0x800000)?value-0x1000000:value)/8388608;break;
      }
      case 's32le':sample=view.getInt32(position,true)/2147483648;break;
      case 'f32le':sample=view.getFloat32(position,true);break;
    }
    result[channel][frame]=Number.isFinite(sample)?Math.max(-1,Math.min(1,sample)):0;
    position+=bytes;
  }
  return result;
}
