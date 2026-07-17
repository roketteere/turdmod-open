#include "TurdMODAuthorVehicleCommandlet.h"

#if WITH_EDITOR
#include "Engine/Blueprint.h"
#include "Engine/BlueprintGeneratedClass.h"
#include "Kismet2/KismetEditorUtilities.h"
#include "UObject/Package.h"
#include "UObject/UObjectGlobals.h"
#include "UObject/UObjectIterator.h"
#include "Misc/PackageName.h"
#include "Misc/Parse.h"
#include "GameFramework/Actor.h"
#include "Engine/SkeletalMesh.h"
#include "Components/SkeletalMeshComponent.h"
#include "UObject/UnrealType.h"
#endif

int32 UTurdMODAuthorVehicleCommandlet::Main(const FString& Params)
{
#if WITH_EDITOR
	// --- 1. Resolve the SCUM parent vehicle class -----------------------------------
	// AAirplane → ADcxWheeledVehicle4W gives mount/seats/replication/flight for free.
	// Not compiled here, so resolve the UClass by path. The cooked BP records this parent
	// by PATH NAME, so in-game UE binds it to SCUM's REAL class at load (same path).
	FString ParentPath = TEXT("/Script/SCUM.Airplane");
	FParse::Value(*Params, TEXT("parent="), ParentPath);
	FString AssetName = TEXT("BP_TurdHeli");
	FParse::Value(*Params, TEXT("name="), AssetName);

	// Package DIRECTORY (mount path). For native vehicle PRIMARY-ASSET registration the
	// BP must live under SCUM's scanned vehicle tree so UConZAssetManager registers it as
	// a "Vehicle" primary asset at boot. Default to /Game/ConZ_Files/Vehicles/TurdMOD/.
	// @inv the cooked asset's package name (baked at author time) is what the scan reads —
	//   pak file-remap alone does NOT relocate the asset for the registry.
	FString PkgDir = TEXT("/Game/ConZ_Files/Vehicles/TurdMOD");
	FParse::Value(*Params, TEXT("path="), PkgDir);
	PkgDir.RemoveFromEnd(TEXT("/"));

	UClass* ParentClass = FindObject<UClass>(nullptr, *ParentPath);                 // already loaded?
	if (!ParentClass)
	{
		ParentClass = StaticLoadClass(AActor::StaticClass(), nullptr, *ParentPath); // borrowed stub / script module
	}
	if (!ParentClass)
	{
		// Last resort: any loaded UClass whose name is the leaf of ParentPath ("Airplane").
		FString Leaf = ParentPath;
		int32 Dot = INDEX_NONE;
		Leaf.FindLastChar(TEXT('.'), Dot);
		if (Dot == INDEX_NONE) { Leaf.FindLastChar(TEXT('/'), Dot); }
		if (Dot != INDEX_NONE) { Leaf = Leaf.RightChop(Dot + 1); }
		for (TObjectIterator<UClass> It; It; ++It)
		{
			if (It->GetName() == Leaf) { ParentClass = *It; break; }
		}
	}

	if (!ParentClass)
	{
		UE_LOG(LogTemp, Error,
			TEXT("[TurdHeli] PARENT '%s' NOT FOUND in the editor — SCUM's Airplane class isn't loaded here. ")
			TEXT("NEXT: drop a borrowed Airplane class-stub uasset into Content/SCUM/ (Dumper-7 / CUE4Parse), ")
			TEXT("or pass -parent=<a loadable class path>. Can't author a vehicle BP without its parent."),
			*ParentPath);
		return 2;
	}
	UE_LOG(LogTemp, Warning, TEXT("[TurdHeli] parent resolved: '%s' -> %s (super=%s)"),
		*ParentPath, *GetNameSafe(ParentClass), *GetNameSafe(ParentClass->GetSuperClass()));

	// --- 2. Create the package + Blueprint ------------------------------------------
	const FString PkgName = FString::Printf(TEXT("%s/%s"), *PkgDir, *AssetName);
	UPackage* Package = CreatePackage(*PkgName);
	if (!Package) { UE_LOG(LogTemp, Error, TEXT("[TurdHeli] CreatePackage failed")); return 1; }
	Package->FullyLoad();

	UBlueprint* BP = FKismetEditorUtilities::CreateBlueprint(
		ParentClass,
		Package,
		FName(*AssetName),
		EBlueprintType::BPTYPE_Normal,
		UBlueprint::StaticClass(),
		UBlueprintGeneratedClass::StaticClass(),
		NAME_None);
	if (!BP) { UE_LOG(LogTemp, Error, TEXT("[TurdHeli] CreateBlueprint failed (parent=%s)"), *GetNameSafe(ParentClass)); return 3; }

	// --- 3. Compile ----------------------------------------------------------------
	FKismetEditorUtilities::CompileBlueprint(BP);

	// --- 3b. Set the vehicle's skeletal mesh on the inherited _vehicleMeshComponent so the
	// native assembler has mount-socket bones to attach the preset's parts to (no mesh -> assembly
	// aborts -> entity id 0). SK_jeep is rigged with FrontLeft_Mount/etc. matching the mount slots.
	FString MeshPath = TEXT("/Game/TurdMOD/SK_jeep.SK_jeep");
	FParse::Value(*Params, TEXT("mesh="), MeshPath);
	if (!MeshPath.IsEmpty() && MeshPath != TEXT("none"))
	{
		USkeletalMesh* Mesh = LoadObject<USkeletalMesh>(nullptr, *MeshPath);
		UObject* CDO = BP->GeneratedClass ? BP->GeneratedClass->GetDefaultObject() : nullptr;
		FObjectProperty* CompProp = CDO ? FindFProperty<FObjectProperty>(BP->GeneratedClass, TEXT("_vehicleMeshComponent")) : nullptr;
		UObject* Comp = CompProp ? CompProp->GetObjectPropertyValue_InContainer(CDO) : nullptr;
		USkeletalMeshComponent* SkComp = Cast<USkeletalMeshComponent>(Comp);
		if (Mesh && SkComp)
		{
			SkComp->SetSkeletalMesh(Mesh);
			UE_LOG(LogTemp, Warning, TEXT("[TurdHeli] set _vehicleMeshComponent.SkeletalMesh = %s"), *MeshPath);
		}
		else
		{
			UE_LOG(LogTemp, Warning, TEXT("[TurdHeli] MESH NOT SET: mesh=%d cdo=%d compProp=%d comp=%s skComp=%d"),
				Mesh ? 1 : 0, CDO ? 1 : 0, CompProp ? 1 : 0, *GetNameSafe(Comp), SkComp ? 1 : 0);
		}
		FKismetEditorUtilities::CompileBlueprint(BP);
	}

	Package->MarkPackageDirty();

	const FString FileName = FPackageName::LongPackageNameToFilename(
		PkgName, FPackageName::GetAssetPackageExtension());
	const bool bSaved = UPackage::SavePackage(Package, BP, RF_Public | RF_Standalone, *FileName);

	UE_LOG(LogTemp, Warning,
		TEXT("[TurdHeli] === %s authored: saved=%d  parent=%s  genClass=%s  file=%s ==="),
		*AssetName, bSaved ? 1 : 0, *ParentPath, *GetNameSafe(BP->GeneratedClass), *FileName);
	// NEXT (after this proves the parent binds): add the heli StaticMesh (SM_TurdHeli) +
	// override the flight params for hover/VTOL on the BP's CDO, then cook (client+server),
	// pak, deploy to F: client + local server, register-spawn via the bridge.
	return bSaved ? 0 : 4;
#else
	UE_LOG(LogTemp, Error, TEXT("[TurdHeli] requires WITH_EDITOR"));
	return 1;
#endif
}
