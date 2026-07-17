// gen-vehicle-rig.mjs — generate a RIGGED glTF (.gltf, base64-embedded) for a custom vehicle:
// a simple body box skinned to a root bone, plus named locator bones for each seat + wheel.
// UE4.27 imports this as a SKELETAL mesh; the named bones become the sockets the mount slots
// (MountSocketName) + wheel configs (BoneName) reference. This is the "rig a model" step for
// building native custom vehicles on SCUM's vehicle framework.
//
// Usage: node gen-vehicle-rig.mjs <jeep|copter> > out.gltf   (or -o file)
// @inv every bone name here must match the MountSocketName / wheel BoneName used in the BP author step.
import fs from 'node:fs';

const kind = (process.argv[2] || 'jeep').toLowerCase();
const outFlag = process.argv.indexOf('-o');
const outPath = outFlag > 0 ? process.argv[outFlag + 1] : null;

// --- vehicle dims (cm, UE units) + bone layout ---------------------------------
// Body box half-extents and the seat/wheel bone positions. Seats ~motorcycle/car height.
const defs = {
  jeep:   { body: [110, 75, 45], bodyZ: 45,
            seats: { FrontLeft_Mount:[40,-38,55], FrontRight_Mount:[40,38,55], BackLeft_Mount:[-45,-38,55], BackRight_Mount:[-45,38,55] },
            wheels:{ Wheel_FL:[80,-72,18], Wheel_FR:[80,72,18], Wheel_BL:[-80,-72,18], Wheel_BR:[-80,72,18] } },
  copter: { body: [120, 60, 50], bodyZ: 60,
            seats: { Driver_Mount:[60,-30,70], Passenger_Mount:[60,30,70] },
            wheels:{ Skid_L:[0,-55,5], Skid_R:[0,55,5] } },
};
const D = defs[kind];
if (!D) { console.error('kind must be jeep or copter'); process.exit(1); }

// --- box geometry (positions + normals + indices), centered, lifted by bodyZ ----
function box(hx, hy, hz, cz) {
  const P = [], N = [], I = [];
  // 6 faces, 4 verts each (flat normals)
  const faces = [
    { n:[ 0, 0, 1], v:[[-hx,-hy,hz],[hx,-hy,hz],[hx,hy,hz],[-hx,hy,hz]] },
    { n:[ 0, 0,-1], v:[[-hx,hy,-hz],[hx,hy,-hz],[hx,-hy,-hz],[-hx,-hy,-hz]] },
    { n:[ 1, 0, 0], v:[[hx,-hy,-hz],[hx,hy,-hz],[hx,hy,hz],[hx,-hy,hz]] },
    { n:[-1, 0, 0], v:[[-hx,hy,-hz],[-hx,-hy,-hz],[-hx,-hy,hz],[-hx,hy,hz]] },
    { n:[ 0, 1, 0], v:[[hx,hy,-hz],[-hx,hy,-hz],[-hx,hy,hz],[hx,hy,hz]] },
    { n:[ 0,-1, 0], v:[[-hx,-hy,-hz],[hx,-hy,-hz],[hx,-hy,hz],[-hx,-hy,hz]] },
  ];
  for (const f of faces) {
    const b = P.length / 3;
    for (const v of f.v) { P.push(v[0], v[1], v[2] + cz); N.push(...f.n); }
    I.push(b, b+1, b+2, b, b+2, b+3);
  }
  return { P, N, I };
}
const g = box(D.body[0], D.body[1], D.body[2], D.bodyZ);
const vcount = g.P.length / 3;

// --- skeleton: bone 0 = Root (skin target), then seats, then wheels --------------
const bones = [{ name: 'Root', t: [0, 0, 0] }];
for (const [nm, t] of Object.entries(D.seats)) bones.push({ name: nm, t });
for (const [nm, t] of Object.entries(D.wheels)) bones.push({ name: nm, t });

// all mesh verts skinned 100% to Root (joint 0)
const JOINTS = new Uint8Array(vcount * 4);   // all 0
const WEIGHTS = new Float32Array(vcount * 4);
for (let i = 0; i < vcount; i++) WEIGHTS[i*4] = 1.0;

// --- pack buffers --------------------------------------------------------------
const pos = new Float32Array(g.P), nrm = new Float32Array(g.N), idx = new Uint16Array(g.I);
// inverseBindMatrices: inverse of each bone's bind world transform (translation only) = translate(-t)
const ibm = new Float32Array(bones.length * 16);
bones.forEach((b, k) => {
  const m = [1,0,0,0, 0,1,0,0, 0,0,1,0, -b.t[0],-b.t[1],-b.t[2],1]; // column-major translate(-t)
  ibm.set(m, k * 16);
});

function align4(n){ return (n + 3) & ~3; }
const parts = [
  { name:'pos', data: Buffer.from(pos.buffer) },
  { name:'nrm', data: Buffer.from(nrm.buffer) },
  { name:'joints', data: Buffer.from(JOINTS.buffer) },
  { name:'weights', data: Buffer.from(WEIGHTS.buffer) },
  { name:'idx', data: Buffer.from(idx.buffer) },
  { name:'ibm', data: Buffer.from(ibm.buffer) },
];
let off = 0; const views = [];
const chunks = [];
for (const p of parts) { const o = align4(off); if (o>off) chunks.push(Buffer.alloc(o-off)); chunks.push(p.data); views.push({ byteOffset:o, byteLength:p.data.length }); off = o + p.data.length; }
const buf = Buffer.concat(chunks);
const b64 = buf.toString('base64');

// bounds for POSITION accessor
let mn=[1e9,1e9,1e9], mx=[-1e9,-1e9,-1e9];
for (let i=0;i<vcount;i++){ for(let c=0;c<3;c++){ const v=g.P[i*3+c]; if(v<mn[c])mn[c]=v; if(v>mx[c])mx[c]=v; } }

// --- glTF JSON -----------------------------------------------------------------
// node tree: scene -> [Armature(node 0) -> Root(1) -> seat/wheel bones], MeshNode(skinned)
const boneNodeBase = 1;                      // bone nodes start at index 1
const meshNodeIndex = boneNodeBase + bones.length;
const nodes = [];
nodes.push({ name:'Armature', children:[boneNodeBase] });                 // 0
bones.forEach((b, k) => {
  const n = { name:b.name, translation:b.t };
  if (k === 0) n.children = bones.map((_,j)=>j).filter(j=>j!==0).map(j=>boneNodeBase+j); // Root parents the rest
  nodes.push(n);
});
nodes.push({ name:`SK_${kind}`, mesh:0, skin:0 });                         // mesh node

const gltf = {
  asset:{ version:'2.0', generator:'turdmod gen-vehicle-rig' },
  scene:0,
  scenes:[{ nodes:[0, meshNodeIndex] }],
  nodes,
  skins:[{ name:`SKEL_${kind}`, joints: bones.map((_,k)=>boneNodeBase+k), skeleton:boneNodeBase, inverseBindMatrices:5 }],
  meshes:[{ name:`SM_${kind}`, primitives:[{ attributes:{ POSITION:0, NORMAL:1, JOINTS_0:2, WEIGHTS_0:3 }, indices:4, material:0 }] }],
  materials:[{ name:`M_${kind}`, pbrMetallicRoughness:{ baseColorFactor:[0.30,0.32,0.30,1], metallicFactor:0.0, roughnessFactor:0.8 } }],
  accessors:[
    { bufferView:0, componentType:5126, count:vcount, type:'VEC3', min:mn, max:mx }, // POSITION
    { bufferView:1, componentType:5126, count:vcount, type:'VEC3' },                  // NORMAL
    { bufferView:2, componentType:5121, count:vcount, type:'VEC4' },                  // JOINTS_0 (u8)
    { bufferView:3, componentType:5126, count:vcount, type:'VEC4' },                  // WEIGHTS_0
    { bufferView:4, componentType:5123, count:idx.length, type:'SCALAR' },            // indices (u16)
    { bufferView:5, componentType:5126, count:bones.length, type:'MAT4' },            // inverseBindMatrices
  ],
  bufferViews: views.map((v,i)=>({ buffer:0, byteOffset:v.byteOffset, byteLength:v.byteLength, ...( i===4 ? {target:34963} : (i<4?{target:34962}:{}) ) })),
  buffers:[{ byteLength: buf.length, uri:`data:application/octet-stream;base64,${b64}` }],
};

const json = JSON.stringify(gltf, null, 1);
if (outPath) { fs.writeFileSync(outPath, json); console.error(`wrote ${outPath} (${vcount} verts, ${bones.length} bones: ${bones.map(b=>b.name).join(', ')})`); }
else process.stdout.write(json);
