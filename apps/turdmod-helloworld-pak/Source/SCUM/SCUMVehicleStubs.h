// SCUMVehicleStubs.h — editor/cook-only stubs of SCUM's vehicle parent chain, so a Blueprint
// can be authored with parent `/Script/SCUM.Airplane`. In-game, the REAL SCUM classes (identical
// class paths) are used; these stubs carry NO UPROPERTYs — the cooked BP binds its parent by
// PATH NAME and reconciles inherited properties by name at load.
//
// Chain (from the dumped SCUM SDK, SCUM_classes.hpp):
//   AAirplane -> ADcxWheeledVehicle4W -> ADcxWheeledVehicle -> AVehicleBase -> APawn (engine)
//
// @inv class NAMES + module name ("SCUM") must match SCUM exactly so paths line up:
//   /Script/SCUM.Airplane, /Script/SCUM.DcxWheeledVehicle4W, etc.
// @inv stubs are NEVER spawned for real — only used as the BP parent at author/cook time.
#pragma once

#include "CoreMinimal.h"
#include "GameFramework/Pawn.h"
#include "Engine/DataAsset.h"
#include "Components/SkeletalMeshComponent.h"
#include "SCUMVehicleStubs.generated.h"

// @inv: _vehicleMeshComponent must match the REAL AVehicleBase._vehicleMeshComponent (a
// UVehicleMeshComponent : USkeletalMeshComponent) by NAME so the cooked SkeletalMesh override
// binds to the real component in-game. The native assembler needs this mesh's mount-socket
// bones (FrontLeft_Mount/etc.) to attach the preset's parts -> no mesh = no sockets = id 0.
UCLASS()
class SCUM_API AVehicleBase : public APawn
{
	GENERATED_BODY()
public:
	AVehicleBase()
	{
		_vehicleMeshComponent = CreateDefaultSubobject<USkeletalMeshComponent>(TEXT("_vehicleMeshComponent"));
		RootComponent = _vehicleMeshComponent;
	}
	UPROPERTY(VisibleAnywhere) USkeletalMeshComponent* _vehicleMeshComponent;
};

// ─── Vehicle-preset (assembly recipe) stubs ──────────────────────────────────
// In-game these bind to the REAL /Script/SCUM.VehiclePreset / .VehiclePresetNode
// by class path; the cooked asset reconciles property values by NAME at load.
// @inv property NAMES + types must match SCUM exactly (SCUM_classes.hpp:86197/86225):
//   VehiclePresetNode: AttachmentClass(TSoftClassPtr), IsFunctionalityAttachment(bool),
//                      SpawnChance(float), Children(TArray<UVehiclePresetNode*>)
//   VehiclePreset:     VehicleClass(TSoftClassPtr), RootNode(UVehiclePresetNode*)
// @ctx AttachmentClass is a SOFT class ref — at author time we set it by PATH STRING to
//   SCUM's existing framework attachment/mount-slot classes; the target class need NOT
//   exist in the cook project (soft refs are just paths resolved in-game).
UCLASS()
class SCUM_API UVehiclePresetNode : public UObject
{
	GENERATED_BODY()
public:
	UPROPERTY(EditAnywhere) TSoftClassPtr<UObject> AttachmentClass;
	UPROPERTY(EditAnywhere) bool IsFunctionalityAttachment = false;
	UPROPERTY(EditAnywhere) float SpawnChance = 100.f;
	UPROPERTY(EditAnywhere, Instanced) TArray<UVehiclePresetNode*> Children;
};

UCLASS()
class SCUM_API UVehiclePreset : public UDataAsset
{
	GENERATED_BODY()
public:
	UPROPERTY(EditAnywhere) TSoftClassPtr<UObject> VehicleClass;
	UPROPERTY(EditAnywhere, Instanced) UVehiclePresetNode* RootNode = nullptr;
};

UCLASS()
class SCUM_API ADcxWheeledVehicle : public AVehicleBase
{
	GENERATED_BODY()
};

UCLASS()
class SCUM_API ADcxWheeledVehicle4W : public ADcxWheeledVehicle
{
	GENERATED_BODY()
};

UCLASS()
class SCUM_API AAirplane : public ADcxWheeledVehicle4W
{
	GENERATED_BODY()
};
