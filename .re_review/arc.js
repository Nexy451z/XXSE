
const fs = require('fs');
const zlib = require('zlib');
function parseArc(buf){
  const magic = buf.toString('latin1',0,4);
  const version = buf.readUInt16LE(4);
  const count = buf.readUInt16LE(6);
  const entries=[];
  for(let i=0;i<count;i++){
    const e=12+80*i;
    let end=buf.indexOf(0,e); if(end<0||end>e+64) end=e+64;
    const name=buf.toString('latin1',e,end);
    const hash=buf.readUInt32LE(e+0x40);
    const csize=buf.readUInt32LE(e+0x44);
    const usize=buf.readUInt32LE(e+0x48)&0x3fffffff;
    const off=buf.readUInt32LE(e+0x4c);
    let data;
    try{ data=zlib.inflateSync(buf.subarray(off,off+csize)); }catch(err){ data=Buffer.alloc(0); }
    entries.push({i,name,hash:hash.toString(16),csize,usize,off,data});
  }
  return {magic,version,count,entries};
}
const file=process.argv[2];
const a=parseArc(fs.readFileSync(file));
console.log(file, a.magic, 'v'+a.version, a.count+' entries');
const mode=process.argv[3];
if(mode==='qdl'){
  const e=a.entries.find(x=>x.name.endsWith('.qdl')||x.hash==='e0bb1c');
  console.log('qdl name',e.name,'len',e.data.length);
  const b=e.data;
  for(let o=8;o+20<=b.length;o+=20){
    const h=b.readUInt32LE(o);
    const nm=b.subarray(o+4,o+20);
    let z=nm.indexOf(0); if(z<0)z=16;
    console.log(o.toString(16), h.toString(16).padStart(8,'0'), JSON.stringify(nm.toString('latin1',0,z)), 'raw='+nm.toString('hex'));
  }
} else if(mode==='sup'){
  const e=a.entries.find(x=>x.hash==='54539aee');
  const b=e.data; console.log('sup len',b.length);
  let nz=0, firstZero=-1;
  for(let i=0;i+4<=b.length-8;i+=4){
    const o=8+i; const item=b.readUInt16LE(o), cnt=b[o+2], slot=b[o+3];
    console.log('rec',i/4,item,cnt,slot);
    if(item===0&&cnt===0&&slot===0&&firstZero<0) firstZero=i/4;
    if(!(item===0&&cnt===0&&slot===0)) nz++;
  }
  console.log('nonzero',nz,'firstZero',firstZero);
} else if(mode==='sem'){
  for(const e of a.entries.filter(x=>x.hash==='2553701d')){
    const b=e.data;
    console.log(e.name, [...Array(8).keys()].map(k=>b.readFloatLE(k*4).toFixed(3)).join(','), 'flag', b.readUInt32LE(0xc));
  }
} else {
  for(const e of a.entries) console.log(e.i, e.name, e.hash, e.usize);
}
