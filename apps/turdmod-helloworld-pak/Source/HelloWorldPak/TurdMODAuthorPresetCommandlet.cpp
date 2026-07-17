#include "TurdMODAuthorPresetCommandlet.h"

#if WITH_EDITOR
#include "SCUMVehicleStubs.h"            // UVehiclePreset stub (binds to /Script/SCUM.VehiclePreset in-game)
#include "UObject/Package.h"
#include "UObject/UObjectGlobals.h"
#include "UObject/SoftObjectPath.h"
#include "Misc/PackageName.h"
#include "Misc/Parse.h"
#endif

int32 UTurdMODAuthorPresetCommandlet::Main(const FString& Params)
{
#if WITH_EDITOR
	FString AssetName = TEXT("VP_TurdJeep");
	FParse::Value(*Params, TEXT("name="), AssetName);
	FString PkgDir = TEXT("/Game/ConZ_Files/Vehicles/TurdMOD");
	FParse::Value(*Params, TEXT("path="), PkgDir);
	PkgDir.RemoveFromEnd(TEXT("/"));
	FString VehiclePath = TEXT("/Game/ConZ_Files/Vehicles/TurdMOD/BP_TurdJeep.BP_TurdJeep_C");
	FParse::Value(*Params, TEXT("vehicle="), VehiclePath);

	const FString PkgName = FString::Printf(TEXT("%s/%s"), *PkgDir, *AssetName);
	UPackage* Package = CreatePackage(*PkgName);
	if (!Package) { UE_LOG(LogTemp, Error, TEXT("[VP] CreatePackage failed")); return 1; }
	Package->FullyLoad();

	UVehiclePreset* Preset = NewObject<UVehiclePreset>(
		Package, UVehiclePreset::StaticClass(), FName(*AssetName), RF_Public | RF_Standalone);
	if (!Preset) { UE_LOG(LogTemp, Error, TEXT("[VP] NewObject<UVehiclePreset> failed")); return 2; }

	// VehicleClass is a soft class ref — set by PATH (target need not be loaded at cook time).
	Preset->VehicleClass = TSoftClassPtr<UObject>(FSoftObjectPath(VehiclePath));
	Preset->RootNode = nullptr;  // wired at runtime to a working stock tree via the bridge

	Package->MarkPackageDirty();
	const FString FileName = FPackageName::LongPackageNameToFilename(
		PkgName, FPackageName::GetAssetPackageExtension());
	const bool bSaved = UPackage::SavePackage(Package, Preset, RF_Public | RF_Standalone, *FileName);

	UE_LOG(LogTemp, Warning,
		TEXT("[VP] === %s authored: saved=%d  VehicleClass=%s  file=%s ==="),
		*AssetName, bSaved ? 1 : 0, *VehiclePath, *FileName);
	return bSaved ? 0 : 4;
#else
	UE_LOG(LogTemp, Error, TEXT("[VP] requires WITH_EDITOR"));
	return 1;
#endif
}
