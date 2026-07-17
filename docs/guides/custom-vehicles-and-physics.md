# Custom Vehicles and Physics for UE 4.27 / SCUM

**Document Version:** 1.0  
**Date:** 2026-05-23  
**Author:** turdmod Vehicle & Physics Specialist (Joel)  

**Scope:** This document covers the complete pipeline for authoring, importing, configuring, and deploying custom vehicles and physics modifications for SCUM using Unreal Engine 4.27.2. It assumes access to the turdmod-content project and the Team’s custom mod pipeline (including the Layer 3 pak-bypass crack, currently in research). Once Layer 3 is resolved, custom paks load natively, enabling any vehicle type UE 4.27 supports.

---

## 1. UE 4.27 Vehicle Authoring Overview

### 1.1 Supported Vehicle Systems in UE 4.27

Unreal Engine 4.27 provides exactly **one** production-ready vehicle system:

- **AWheeledVehicle** + **UWheeledVehicleMovementComponent** (PhysX-based)  
- **Chaos Vehicle** is **not available** – that is an UE5 feature (5.0+).  
- **UWheeledVehicleMovementComponent4W** is the default implementation for 4‑wheeled cars.  
- Custom movement components can be written in C++ (or via blueprint subclassing) to support hover, boat, aircraft, etc.

**Key classes:**  

| Class | Path (UE 4.27) | Purpose |
|-------|----------------|---------|
| `AWheeledVehicle` | `Engine/Classes/Vehicles/WheeledVehicle.h` | Base actor for wheeled vehicles |
| `UWheeledVehicleMovementComponent` | `Engine/Classes/Vehicles/WheeledVehicleMovementComponent.h` | Movement logic, engine, transmission, suspension, steering |
| `UVehicleWheel` | `Engine/Classes/Vehicles/VehicleWheel.h` | Wheel setup (radius, suspension, friction) |
| `UWheeledVehicleMovementComponent4W` | `Engine/Classes/Vehicles/WheeledVehicleMovementComponent4W.h` | 4‑wheel defaults (steering, differential, etc.) |

**VPhysics (Vehicle Physics) Module:** Enabled by default – do not disable.

**Tire Model:** PhysX’s PxVehicleDrive4W / PxVehicleNoDrive is used internally. Key parameters:  
- **Longitudinal stiffness** (grip when accelerating/braking)  
- **Lateral stiffness** (grip when cornering)  
- **Camber stiffness**  
- **Friction** (material‑based per surface)  

**Suspension:**  
- Spring rate, damping rate, rest length, max raise/compress  
- Suspension travel defined per wheel  

**Engine & Transmission:**  
- Torque curve (RPM → torque)  
- Gear ratios, reverse ratio  
- Differential type (open, limited‑slip, locked)  

**Center of Gravity (COG):**  
- Set via `CMassOverride` in PhysicsAsset or via `CenterOfMassOffset` in movement component.  
- `UWheeledVehicleMovementComponent` exposes `COMOverride` bool and `COMOffset` vector.

### 1.2 Creating a Wheeled Vehicle – Step by Step Menu Paths

1. **Create a new Blueprint**  
   `Content Browser > Right Click > Blueprint Class > Pick Parent Class` → choose `WheeledVehicle` (or any existing SCUM vehicle base – see Section 2).  
   *Alternatively, derive from a turdmod‑provided base (e.g., `TurboVehicleBase`) that already handles replication and Steam IDs.*

2. **Add Wheels**  
   The default `AWheeledVehicle` spawns 4 wheels by default. To customise:  
   - Open `WheeledVehicleMovementComponent` (component list).  
   - For each wheel, expand `WheelSetups` array.  
   - Create a new `VehicleWheel` Blueprint (`Content Browser > Create > Advanced Assets > Blueprint > Blueprint` – base class `VehicleWheel`).  
   - Assign each wheel blueprint to a `WheelClass` entry.

3. **Configure Wheel Blueprint**  
   Open the wheel blueprint. Properties you must set:  
   - `WheelRadius` – in cm (e.g., 40 for a car, 25 for a small bike)  
   - `WheelWidth` – affects lateral friction area  
   - `SuspensionForceOffset` – lateral offset of spring  
   - `SuspensionMaxRaise` / `SuspensionMaxDrop` – travel limits  
   - `SuspensionNaturalFrequency` – (rad/s) typical 7.0 – 15.0  
   - `SuspensionDampingRatio` – typical 0.2 – 0.8  
   - `WheelMass` – kg  
   - `FrictionForceMultiplier` – scale of friction  
   - `SteerAngle` – max steering in degrees  
   - `MaxBrakeTorque`

4. **Configure Engine & Transmission**  
   Select the `WheeledVehicleMovementComponent` in the Details panel:  
   - `Engine Setup`  
     - `MaxRPM` (e.g., 6000)  
     - `MOI` (Moment of inertia) – higher = slower rev changes  
     - `DampingRateFullThrottle` / `DampingRateZeroThrottle` / `DampingRateClutchEngaged`  
     - `TorqueCurve` – an `FRuntimeFloatCurve`. Open curve editor: `RPM` from 0..1 (normalised) to `Torque` (Nm).  
   - `Transmission Setup`  
     - `NeutralGearUpRatio` – engine revs when shifting up  
     - `ClutchStrength` – 0..1, slip amount  
     - `ForwardGears` – array of gear ratios (e.g., 4.0, 2.5, 1.66, 1.23, 0.85)  
     - `ReverseGearRatio` – e.g., -4.0  
   - `Steering Setup`  
     - `SteeringCurve` – speed vs steering angle multiplier  
   - `Differential Setup`  
     - `DifferentialType` – `LimitedSlip_4W` is common. Front/Rear bias.  

5. **Add Collision & Physics Asset**  
   See Section 4.

6. **Add Audio**  
   - Create an `AudioComponent` childed to the root.  
   - Use `SetIntParameter` or `SetFloatParameter` with the exposed `EngineRPM` and `Speed` from the movement component’s generated events.  
   - For example: `Event OnRPMChanged (new RPM) → set RTPC on AudioComponent`.

7. **Compile & Save** – your vehicle blueprint is ready.

---

## 2. SCUM’s Vehicle Inheritance

SCUM already uses a sophisticated vehicle system based on `VehicleBase` (C++ parent). All in‑game vehicles derive from it:

- `BPC_KingletDuster_C`  
- `BPC_Cruiser_C`  
- `BPC_Wolfsy_C`  
- `BPC_Rager_C`  
- `BPC_Tractor_C`  
- `BPC_Bicycle_C` (two‑wheeled)  
- etc.

Key observations from previous reverse‑engineering:  
- Vehicles store `_driver` at **offset 0x12F8** (likely a TWeakObjectPtr).  
- Replication is handled via `_repServerEntitySetupAndId` and `NetMulticast_PlayInstantDestructionEffectsAtLocation`.  
- The vehicle class hierarchy is: `AActor → VehicleBase → (SCUM’s custom BP vehicles)`.

**Why custom vehicles work:**  
SCUM’s engine does **not** hardcode vehicle class names. The pak loader accepts any `AActor` subclass that exists within loaded packages. As long as the pak is recognised (via Layer 3 crack), the server and client will instantiate the new blueprint. The base class can be `VehicleBase` (recommended) to inherit replication and damage handling, or any other `AWheeledVehicle` derived class.

**To create a custom vehicle for SCUM:**  
1. Derive from `VehicleBase` (represents the existing SCUM base – you may need to include the header from the modded `.h` file).  
2. Add `WheeledVehicleMovementComponent` (or a custom movement component).  
3. Override `BeginPlay`, `Tick`, and relevant networking events.  

Alternatively, use the turdmod‑provided base `TU_VehicleBase_C` which already implements the necessary replication boilerplate and exposes hooks for admin‑spawn (`!mech bring`). (This base will be included in turdmod‑content v2.1+.)

---

## 3. Custom Vehicle Authoring Recipe (Checklist)

### 3.1 Prerequisites

- Unreal Engine 4.27.2 installed (recommend via Epic Games Launcher).  
- The `turdmod-content` project open (this is a mod‑friendly UE project that includes all SCUM engine plugins and the necessary `.uproject` plugins).  
- Access to the `Layer 3` bypass tool (currently in research – without it you can only test in PIE mode).

### 3.2 Step‑by‑Step Checklist

- [ ] 1. Import vehicle mesh (FBX with skeleton if animated, else static).  
- [ ] 2. Create a new Blueprint class: `VehicleBase` (or `TU_VehicleBase`).  
- [ ] 3. Assign the mesh to the `SkeletalMeshComponent` (or `StaticMeshComponent` if you want simpler physics – but wheeled vehicles require skeletal for wheel bone animation).  
- [ ] 4. Create PhysX Shape for the chassis (see Section 4).  
- [ ] 5. Create 1‑4 (or more) wheel subclass blueprints (`VehicleWheel`).  
- [ ] 6. Add `WheeledVehicleMovementComponent` to the vehicle actor.  
- [ ] 7. Populate `WheelSetups` array with references to wheel blueprints.  
- [ ] 8. Configure Engine, Transmission, Steering, Diff as described in 1.2.  
- [ ] 9. Tune suspension, friction, mass.  
- [ ] 10. Add Audio – create two `AudioComponent`s (engine + tyre). Wire RPM and speed to parameter curves.  
- [ ] 11. Implement `OnVehicleDestroyed` / `OnTakeDamage` from SCUM’s `VehicleBase` (if replicating SCUM’s destruction effects, call `NetMulticast_PlayInstantDestructionEffectsAtLocation`).  
- [ ] 12. Test in PIE (single player) – drive around, check handling.  
- [ ] 13. Package and deploy as **two separate paks**: one for client, one for server (identical content).  
- [ ] 14. Upload to turdmod‑marketplace.

### 3.3 Cooking & Pak Creation

**Cook command (Project Launcher or command line):**  
```
Engine\Binaries\Win64\UE4Editor-Cmd.exe "C:\Path\To\turdmod-content.uproject" -run=Cook -targetplatform=WindowsNoEditor -OutputDir="C:\CookedOutput" -ddc=None -cookonthefly -Unversioned
```

**Create a pak file using UnrealPak.exe:**  
```
Engine\Binaries\Win64\UnrealPak.exe "D:\Output\MyVehicle_P.pak" -Create="ListOfFiles.txt" -Compress
```

**ListOfFiles.txt example:**  
```
../../../CookedOutput/WindowsNoEditor/turdmod-content/Content/Vehicles/MyVehicle/Car.uasset  "../../../CookedOutput/WindowsNoEditor/turdmod-content/Content/Vehicles/MyVehicle/Car.uexp"
../../../CookedOutput/WindowsNoEditor/turdmod-content/Content/Vehicles/MyVehicle/Wheel_MyWheel.uasset ...
```

**Pak naming convention:** `P_1234_Vehicle_Awesome.pak` (prefix `P_` for mod paks, numeric ID, then description). The Layer 3 loader expects this naming.

---

## 4. Physics Asset Authoring

### 4.1 Opening PhysicsAsset Editor

1. In Content Browser, right‑click the skeletal mesh → **Create > Physics Asset**.  
2. The editor opens with auto‑generated bodies and constraints.  
3. **For vehicles, you typically need only one body** (the chassis) and optionally wheel constraints (if you want to simulate wheelbones as separate bodies – but the built‑in wheel system handles wheel physics via the movement component, not via constraints).  

### 4.2 Setting Up Chassis Collision

- Delete all auto‑generated bodies except the root bone (e.g., `Root` or `Chassis`).  
- Add a **Box** collision: detail panel → `Add Box` and adjust dimensions to enclose the mesh (leave a small gap).  
- Alternatively use a **Convex** hull: `Add Convex` then adjust vertices.  
- Set **Mass** and **Center of Mass Offset**:  
  - In the body details panel: `Mass > 0.0` (will compute density if 0).  
  - `COM > Local Offset` to simulate low COG (e.g., `Z = -30 cm` for cars).  
- Set **Physics Properties**: `Restitution` (bounce), `Friction`, `LinearDamping`, `AngularDamping`.  

### 4.3 Constraint Setup for Suspension (Optional)

If you want separate wheel bodies (for visual or physical effects):  
- Each wheel bone should have its own body with a **Capsule** collision.  
- Create a **Constraint** connecting wheel to chassis bone.  
- Set constraint to **Hinge** (single axis) – allow free rotation on the wheel’s local X (axle).  
- Add **Angular Motor** to drive rotation (motor torque from movement component is not automatically applied – you would need custom C++).  
**Recommendation**: For 99% of vehicles, stick with UE’s internal wheel simulation and only use the chassis body. Save complexity.

### 4.4 Per‑Bone Shape Collision

For complex damage/weld behaviours (e.g., bumpers falling off), you can add multiple bodies with breakable constraints. But for initial vehicle release, the chassis alone is sufficient.

---

## 5. “Almost Any Vehicle Imaginable” – What Fits

### 5.1 Wheeled Vehicles (Standard)

- Cars, trucks, motorcycles, bicycles – all use `WheeledVehicleMovementComponent`.  
- Number of wheels can be 2, 3, 4, 6, 8, etc. – just add more `WheelSetups` entries.  
- For vehicles with more than 4 wheels, you must set `bUseAutoDrive` to false and implement custom steering logic for additional axles.  

### 5.2 Hover Vehicles

- Use a **custom movement component** (derived from `UMovementComponent`).  
- Implement `TickComponent`: apply forces upward based on ground distance (trace downward), plus thrust for forward.  
- No wheel simulation. Replicate via `ReplicatedMovement` or custom RPCs.  
- Requires C++ or blueprint node calling `AddForce` / `AddTorque`.  

### 5.3 Boats

- Use `UWaterMovementComponent` (part of Chaos physics? **Not available in 4.27**).  
- Fallback: custom movement component + `BuoyancyComponent` (from engine content: `Engine/Content/Buoyancy` – does it exist? Check `Plugins/WaterMovement`).  
- Simulate buoyancy via per‑point floatation forces.  

### 5.4 Aircraft

- Fixed‑wing: Use `UFloatingPawnMovement` (aircraft with speed = lift) or `UPlaneMovementComponent`.  
- Helicopter: custom rotor lift + torque via `AddForce` each tick.  
- No wheeled movement; disable `WheeledVehicleMovement` entirely.  

### 5.5 Tanks

- Tracked vehicles: Use wheeled vehicles but set all wheels to **non‑steering** (steer angle 0).  
- Add **yaw control** by applying torque from an extra `CharacterMovementComponent` or custom code that simulates track braking.  
- Turret rotation: child the turret mesh to the chassis with a `RotatingMovementComponent` or `SceneComponent` with rotation updated from input.  

### 5.6 Mechs

- Legs: use `SkeletalMeshComponent` with `UAnimInstance` driven by IK (immersive).  
- Movement: Derived from `APawn` with `UCharacterMovementComponent` – jumping, walking, etc.  
- Do **not** use wheeled vehicle. Can still have a physics chassis with joint constraints to legs.  

### 5.7 Trains

- Follow spline path: use `USplineComponent` and `Velocity = SplineTangent * speed`.  
- Articulated carriages: child actors linked with `PhysicsConstraint` (hinge on yaw).  
- Not wheeled per se – can use wheel visuals but movement is path‑based.  

### 5.8 Submarines

- 3D movement (6 DoF). Use `UFloatingPawnMovement` modified for water drag.  
- Add buoyancy, depth control.  

---

## 6. Physics Modification Examples

These can be applied globally (console) or per‑actor via admin commands.

### 6.1 Floaty Gravity (Low Gravity Mode)

**Console command (server side):**  
```
PhysicsOverride.GravityZ -200
```
Resets to default of `-980` via:  
```
PhysicsOverride.GravityZ -980
```

**Blueprint per‑vehicle:**  
`SetActorEnableGravity(false)` + custom upward force.

### 6.2 Bouncy Physics

Set per‑body `Restitution` to 1.5 (bounce). In PhysicsAsset, select chassis body → `Restitution = 1.5`. For runtime:  
`ChassisBody->SetPhysicsRestitution(1.5f)` via C++.

### 6.3 No‑Friction Ice

Set `Friction` of chassis body to 0.0. Also override the wheel friction: in the wheel blueprint, set `FrictionForceMultiplier = 0.0` (or extremely low). For per‑surface friction, use **Physical Materials** (see PhysicalMaterial in Content Browser → `Friction` = 0.0).

### 6.4 Custom Wind Force

Attach a `RadialForceComponent` or `ConstantForceComponent`.  
For global wind, use `SetWind` in the Level Blueprint: `SetWind( Direction, Strength )` – but that affects all physics objects. For per‑vehicle, spawn an `AreaWind` component.

---

## 7. Integration with the Persona Fleet

### 7.1 Mechanic Persona

- Command `!mech bring <vehicleID>` should spawn the custom vehicle at the crosshair location.  
- Implementation: the mechanic reads the market database, fetches the vehicle class name (e.g., `BPC_CustomRacer_C`), and calls `SpawnActor` with `VehicleBase` as class.  
- The turdmod backend (Layer 3) must ensure the pak is loaded before spawn.  

### 7.2 Architect Persona

- Can place vehicles in the world during session building **(with admin spawn permissions)**.  
- Collision‑modified vehicles (e.g., low friction, high mass) for raid puzzles – set via `SetActorScale3D` or custom property.  

### 7.3 Storyteller Persona

- Trigger physics events via `Event Manager`. Example:  
  - **Event:** “Storm Starts” → fires custom event that sets global wind, reduces friction, pushes vehicles.  
  - Use `AddForce` on all vehicles in a volume.  

---

## 8. Marketplace Distribution

- Vehicle paks are uploaded to the **turdmod‑marketplace** interface alongside widget paks.  
- Each pak is labelled with its tier: **Free**, **Premium‑Included** (with active subscription), **Premium‑Exclusive** (only purchasable by premium members).  
- **Pricing:** Base price per vehicle (free – $5).  
- **Showcase:** Each market entry must include:  
  - A **30‑second video** (in‑game test drive)  
  - Screenshots of the vehicle from 4 angles  
  - Type tags (e.g., `car`, `hover`, `tank`)  
- **Downloads:** After purchase/download, the player places the `.pak` file in the mod directory.  

---

## 9. Performance Considerations

### 9.1 Wheel Count vs. Performance

- Each wheel invokes PhysX sub‑stepping. Default sub‑steps: 2.  
- For **50 vehicles** with 4 wheels each = 200 wheel sims per step.  
- Recommended maximum simultaneous vehicles:  
  - Low‑end: 10 vehicles  
  - Mid‑range: 30  
  - High‑end: 60+ (with LOD).  

### 9.2 Sub‑Step Recommendations

In project settings:  
`Physics > PhysX > Sub Step Count` default 2. For many vehicles, increase to **4** (smoother but costly).  
`Max Sub Step Delta Time` = 1/60 (33ms).  

### 9.3 LOD Strategy

- Use `VehicleDistanceLOD` (built‑in in `UWheeledVehicleMovementComponent`). Set `LODDistanceForSuspension`, `LODDistanceForTireLoad`, etc.  
- For vehicles farther than 500m: disable wheel physics visuals completely (simulate only for replication).  

### 9.4 Replication Culling

- Use `ReplicationGraph`’s `SpatializationCullDistance` – set per vehicle class. Example: cars < 300m, trucks < 500m.  
- For vehicles owned by the server (AI), replicate only to players within hearing distance.  

---

## 10. Risk / Limit Catalog

### 10.1 Primary Blocker: Layer 3

Custom paks will not load until the Layer 3 bypass is complete. This is the **critical path**. Without it, vehicles can only be tested in PIE (single‑player editor).  

### 10.2 Anti‑Cheat Flagging

SCUM currently uses **BattlEye**? In the modded environment, BattlEye is disabled (Joel’s policy – requires launcher bypass). Even so, the server’s original integrity checks may flag custom actors. Solution: Ship the exact same content on both client and server. The Layer 3 crack already bypasses initial pak signature checks.

### 10.3 Mass / Collision Desync

If client and server calculate mass differently (e.g., via PhysicsAsset instead of movement component), the vehicle will behave differently. **Critical rule:** Always set mass via the movement component’s `TotalMassOverride` (or `COMOverride`). The physics asset mass is ignored when the movement component is active.

### 10.4 Wheel Collision with SCUM’s Terrain

SCUM’s terrain collision may have unusual physical materials (grass, mud, road). Test the vehicle on all terrain types. If wheels sink into the ground, adjust `SuspensionMaxDrop` and `WheelRadius`. If the vehicle bounces, increase `SuspensionDampingRatio` to 0.8.

### 10.5 Server Authoritative Movement

All vehicle movement is resolved on server. Ensure your custom movement component is replicated correctly (mark `bReplicates = true`, implement `GetLifetimeReplicatedProps`). For wheeled vehicles, the built‑in component handles replication. For custom (hover/boat), you must manually replicate position and rotation.

---

**End of Document**

*This guide will be updated as the Layer 3 bypass stabilises and more vehicle classes are tested. For questions, contact Joel via Team Discord channel #vehicles-physics.*