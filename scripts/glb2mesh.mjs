import fs from 'fs';
const buf = fs.readFileSync('C:/Development/Claude/turdmod/brand/heli.glb');
// GLB: 12B header, then chunks (len u32, type u32, data)
if (buf.readUInt32LE(0)!==0x46546C67) throw new Error('not glTF');
let off=12, json=null, bin=null;
while(off < buf.length){ const len=buf.readUInt32LE(off); const type=buf.readUInt32LE(off+4); const data=buf.subarray(off+8, off+8+len);
  if(type===0x4E4F534A) json=JSON.parse(data.toString('utf8')); else if(type===0x004E4942) bin=data; off+=8+len; }
const g=json;
const CT={5120:Int8Array,5121:Uint8Array,5122:Int16Array,5123:Uint16Array,5125:Uint32Array,5126:Float32Array};
const NC={SCALAR:1,VEC2:2,VEC3:3,VEC4:4,MAT4:16};
function acc(i){ const a=g.accessors[i]; const bv=g.bufferViews[a.bufferView]; const TA=CT[a.componentType]; const nc=NC[a.type];
  const start=(bv.byteOffset||0)+(a.byteOffset||0); const count=a.count*nc;
  // assume tightly packed (poly.pizza glb)
  const arr=new TA(bin.buffer, bin.byteOffset+start, count); return {arr,nc,count:a.count}; }
// node transforms (compose matrices)
function mat(n){ if(n.matrix) return n.matrix.slice();
  const t=n.translation||[0,0,0], r=n.rotation||[0,0,0,1], s=n.scale||[1,1,1];
  const [x,y,z,w]=r; const x2=x+x,y2=y+y,z2=z+z; const xx=x*x2,xy=x*y2,xz=x*z2,yy=y*y2,yz=y*z2,zz=z*z2,wx=w*x2,wy=w*y2,wz=w*z2;
  return [ (1-(yy+zz))*s[0],(xy+wz)*s[0],(xz-wy)*s[0],0,  (xy-wz)*s[1],(1-(xx+zz))*s[1],(yz+wx)*s[1],0,
           (xz+wy)*s[2],(yz-wx)*s[2],(1-(xx+yy))*s[2],0,  t[0],t[1],t[2],1 ]; }
function mul(a,b){ const o=new Array(16); for(let r=0;r<4;r++)for(let c=0;c<4;c++){let v=0;for(let k=0;k<4;k++)v+=a[k*4+c]*b[r*4+k];o[r*4+c]=v;} return o; }
function xform(m,p){ return [ m[0]*p[0]+m[4]*p[1]+m[8]*p[2]+m[12], m[1]*p[0]+m[5]*p[1]+m[9]*p[2]+m[13], m[2]*p[0]+m[6]*p[1]+m[10]*p[2]+m[14] ]; }
const verts=[], tris=[];
function addPrim(prim, M){ if(prim.attributes.POSITION===undefined) return;
  const pos=acc(prim.attributes.POSITION); const base=verts.length/3;
  for(let i=0;i<pos.count;i++){ const p=xform(M,[pos.arr[i*3],pos.arr[i*3+1],pos.arr[i*3+2]]); verts.push(p[0],p[1],p[2]); }
  let idx; if(prim.indices!==undefined){ const a=acc(prim.indices); idx=a.arr; } else { idx=[...Array(pos.count).keys()]; }
  for(let i=0;i+2<idx.length;i+=3){ tris.push(base+idx[i], base+idx[i+1], base+idx[i+2]); } }
const scene=g.scenes[g.scene||0];
function walk(ni, parent){ const n=g.nodes[ni]; const M=mul(parent, mat(n));
  if(n.mesh!==undefined) for(const prim of g.meshes[n.mesh].primitives) addPrim(prim,M);
  if(n.children) for(const c of n.children) walk(c,M); }
const I=[1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1];
for(const ni of scene.nodes) walk(ni,I);
// bbox -> center + uniform scale to target longest dim
let mn=[1e9,1e9,1e9], mx=[-1e9,-1e9,-1e9];
for(let i=0;i<verts.length;i+=3)for(let k=0;k<3;k++){mn[k]=Math.min(mn[k],verts[i+k]);mx[k]=Math.max(mx[k],verts[i+k]);}
const ctr=[(mn[0]+mx[0])/2,(mn[1]+mx[1])/2,(mn[2]+mx[2])/2];
const dim=Math.max(mx[0]-mn[0],mx[1]-mn[1],mx[2]-mn[2]);
const TARGET=900; const s=TARGET/Math.max(1e-6,dim);   // ~9m longest -> heli-ish
// glTF Y-up RH -> UE Z-up LH: UE=(x, -z, y); plus scale s, centered, sit above ground (+Z)
const out=[];
out.push('V '+(verts.length/3));
for(let i=0;i<verts.length;i+=3){ const x=(verts[i]-ctr[0])*s, y=(verts[i+1]-ctr[1])*s, z=(verts[i+2]-ctr[2])*s;
  out.push(`${x.toFixed(2)} ${(-z).toFixed(2)} ${(y).toFixed(2)}`); }
out.push('F '+(tris.length/3));
for(let i=0;i<tris.length;i+=3) out.push(`${tris[i]} ${tris[i+1]} ${tris[i+2]}`);
fs.writeFileSync('C:/Development/Claude/turdmod/brand/heli.mesh', out.join('\n'));
console.log('verts='+(verts.length/3)+' tris='+(tris.length/3)+' dim='+dim.toFixed(2)+' scale='+s.toFixed(2)+' -> brand/heli.mesh');
