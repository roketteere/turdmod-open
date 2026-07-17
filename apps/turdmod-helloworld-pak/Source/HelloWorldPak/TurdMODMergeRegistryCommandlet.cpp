#include "TurdMODMergeRegistryCommandlet.h"

#if WITH_EDITOR
#include "AssetRegistryModule.h"
#include "AssetRegistry/IAssetRegistry.h"
#include "AssetRegistry/AssetRegistryState.h"
#include "Misc/FileHelper.h"
#include "Serialization/MemoryReader.h"
#include "Serialization/MemoryWriter.h"
#include "Misc/Parse.h"
#endif

int32 UTurdMODMergeRegistryCommandlet::Main(const FString& Params)
{
#if WITH_EDITOR
	FString BasePath, OutPath;
	FString ScanDir = TEXT("/Game/ConZ_Files/Vehicles/Car/TurdJeep");
	FParse::Value(*Params, TEXT("base="), BasePath);
	FParse::Value(*Params, TEXT("out="), OutPath);
	FParse::Value(*Params, TEXT("scan="), ScanDir);
	if (BasePath.IsEmpty() || OutPath.IsEmpty())
	{
		UE_LOG(LogTemp, Error, TEXT("[MergeReg] need -base=<file> -out=<file>"));
		return 2;
	}

	IAssetRegistry& AR = FModuleManager::LoadModuleChecked<FAssetRegistryModule>("AssetRegistry").Get();

	// FULL data options (deps + searchable-name deps + manage deps + package data, no filters)
	// so EVERYTHING in the base registry round-trips. The earlier game/filtered options dropped
	// dependency/package data SCUM needs during client-join replication -> hard crash on join.
	FAssetRegistrySerializationOptions SaveOpts;
	SaveOpts.ModifyForDevelopment();

	// 1. Make sure OUR vehicle asset is in the editor registry.
	AR.ScanPathsSynchronous({ ScanDir }, /*bForceRescan*/ true);

	// 2. Load the base game registry from the extracted .bin (Load reads the version header itself).
	TArray<uint8> BaseBytes;
	if (!FFileHelper::LoadFileToArray(BaseBytes, *BasePath))
	{
		UE_LOG(LogTemp, Error, TEXT("[MergeReg] cannot read base registry: %s"), *BasePath);
		return 3;
	}
	FAssetRegistryState BaseState;
	{
		FMemoryReader Reader(BaseBytes, /*bIsPersistent*/ true);
		FAssetRegistryLoadOptions LoadOpts(SaveOpts);
		if (!BaseState.Load(Reader, LoadOpts))
		{
			UE_LOG(LogTemp, Error, TEXT("[MergeReg] BaseState.Load failed"));
			return 4;
		}
	}
	UE_LOG(LogTemp, Warning, TEXT("[MergeReg] base registry loaded: %d assets"),
		BaseState.GetObjectPathToAssetDataMap().Num());

	// 3. Append the base assets into the editor registry (editor AR now = ours + base).
	AR.AppendState(BaseState);

	// 4. Snapshot the full combined registry.
	FAssetRegistryState Combined;
	AR.InitializeTemporaryAssetRegistryState(Combined, SaveOpts);
	UE_LOG(LogTemp, Warning, TEXT("[MergeReg] combined registry: %d assets"),
		Combined.GetObjectPathToAssetDataMap().Num());

	// sanity: confirm our asset is present
	const FAssetData* Ours = Combined.GetAssetByObjectPath(
		FName(TEXT("/Game/ConZ_Files/Vehicles/Car/TurdJeep/BPC_TurdJeep.BPC_TurdJeep")));
	UE_LOG(LogTemp, Warning, TEXT("[MergeReg] BPC_TurdJeep in combined registry: %s"),
		Ours ? TEXT("YES") : TEXT("NO"));

	TArray<uint8> OutBytes;
	{
		FMemoryWriter Writer(OutBytes, /*bIsPersistent*/ true);
		Combined.Save(Writer, SaveOpts);   // Save writes the version header itself
	}
	if (!FFileHelper::SaveArrayToFile(OutBytes, *OutPath))
	{
		UE_LOG(LogTemp, Error, TEXT("[MergeReg] cannot write merged registry: %s"), *OutPath);
		return 5;
	}
	UE_LOG(LogTemp, Warning, TEXT("[MergeReg] === merged registry written: %s (%d bytes) ==="),
		*OutPath, OutBytes.Num());
	return 0;
#else
	UE_LOG(LogTemp, Error, TEXT("[MergeReg] requires WITH_EDITOR"));
	return 1;
#endif
}
