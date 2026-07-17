# gen-vehicle-blender.py — headless Blender: build + rig + export a custom vehicle as FBX for UE.
# A body box skinned to a Root bone, plus named locator bones for each seat + wheel. UE imports
# the FBX as a SKELETAL mesh; the named bones are the sockets the mount slots (MountSocketName) +
# wheel configs (BoneName) reference.
#   blender --background --python gen-vehicle-blender.py -- <jeep|copter> <out.fbx>
# Units: Blender meters = UE cm/100 (FBX export -> cm -> UE 1:1). @inv bone names == BP MountSocketName/BoneName.
import bpy, sys, os, addon_utils

argv = sys.argv
argv = argv[argv.index("--")+1:] if "--" in argv else []
kind = (argv[0] if argv else "jeep").lower()
out  = argv[1] if len(argv) > 1 else f"C:/Development/Claude/turdmod/apps/turdmod-helloworld-pak/RigSource/SK_{kind}.fbx"

DEFS = {
    "jeep": {"body":[1.10,0.75,0.45], "bodyZ":0.45,
             "seats":{"FrontLeft_Mount":[0.40,-0.38,0.55],"FrontRight_Mount":[0.40,0.38,0.55],
                      "BackLeft_Mount":[-0.45,-0.38,0.55],"BackRight_Mount":[-0.45,0.38,0.55]},
             "wheels":{"Wheel_FL":[0.80,-0.72,0.18],"Wheel_FR":[0.80,0.72,0.18],
                       "Wheel_BL":[-0.80,-0.72,0.18],"Wheel_BR":[-0.80,0.72,0.18]}},
    "copter": {"body":[1.20,0.60,0.50], "bodyZ":0.60,
               "seats":{"Driver_Mount":[0.60,-0.30,0.70],"Passenger_Mount":[0.60,0.30,0.70]},
               "wheels":{"Skid_L":[0.0,-0.55,0.05],"Skid_R":[0.0,0.55,0.05]}},
}
D = DEFS[kind]

# make sure the FBX exporter is available
try: addon_utils.enable("io_scene_fbx", default_set=True, persistent=True)
except Exception as e: print("fbx addon enable note:", e)

bpy.ops.wm.read_factory_settings(use_empty=True)

# --- body mesh (box) ---
bpy.ops.mesh.primitive_cube_add(size=2.0, location=(0,0,D["bodyZ"]))
body = bpy.context.active_object
body.scale = (D["body"][0], D["body"][1], D["body"][2])
bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
body.name = f"SM_{kind}"
mat = bpy.data.materials.new(f"M_{kind}")
mat.diffuse_color = (0.30, 0.32, 0.30, 1.0)
body.data.materials.append(mat)

# --- armature: Root (skin) + named seat/wheel bones ---
arm_data = bpy.data.armatures.new(f"SKEL_{kind}")
arm = bpy.data.objects.new(f"Armature_{kind}", arm_data)
bpy.context.collection.objects.link(arm)
bpy.context.view_layer.objects.active = arm
bpy.ops.object.mode_set(mode='EDIT')
eb = arm_data.edit_bones
root = eb.new("Root"); root.head=(0,0,0); root.tail=(0,0,0.20)
def addbone(name, t):
    b = eb.new(name); b.head=(t[0],t[1],t[2]); b.tail=(t[0],t[1],t[2]+0.12); b.parent=root
for n,t in D["seats"].items():  addbone(n,t)
for n,t in D["wheels"].items(): addbone(n,t)
bpy.ops.object.mode_set(mode='OBJECT')

# --- skin body 100% to Root ---
bpy.ops.object.select_all(action='DESELECT')
body.select_set(True); arm.select_set(True)
bpy.context.view_layer.objects.active = arm
bpy.ops.object.parent_set(type='ARMATURE_NAME')   # armature modifier + per-bone empty vgroups
vg = body.vertex_groups.get("Root") or body.vertex_groups.new(name="Root")
vg.add([v.index for v in body.data.vertices], 1.0, 'REPLACE')

# --- export FBX (UE-friendly) ---
os.makedirs(os.path.dirname(out), exist_ok=True)
bpy.ops.object.select_all(action='SELECT')
bpy.ops.export_scene.fbx(
    filepath=out, use_selection=True, object_types={'ARMATURE','MESH'},
    add_leaf_bones=False, bake_anim=False, mesh_smooth_type='FACE',
    apply_unit_scale=True, global_scale=1.0, primary_bone_axis='Y', secondary_bone_axis='X',
    axis_forward='X', axis_up='Z',
)
bones = ["Root"] + list(D["seats"].keys()) + list(D["wheels"].keys())
print(f"WROTE_FBX {out} bones={','.join(bones)}")
