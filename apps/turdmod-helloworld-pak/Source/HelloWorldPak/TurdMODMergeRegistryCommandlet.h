// TurdMODMergeRegistryCommandlet — produce a MERGED SCUM/AssetRegistry.bin = base game
// registry + our BPC_TurdJeep, so our pak can SHADOW the baked registry and the AssetManager
// registers our vehicle as a "Vehicle" primary asset at boot.
//
// Run: UE4Editor-Cmd <uproject> -run=TurdMODMergeRegistry -nullrhi -unattended
//      -base=<extracted base AssetRegistry.bin> -out=<merged output>
//      [ -scan=/Game/ConZ_Files/Vehicles/Car/TurdJeep ]
#pragma once

#include "CoreMinimal.h"
#include "Commandlets/Commandlet.h"
#include "TurdMODMergeRegistryCommandlet.generated.h"

UCLASS()
class UTurdMODMergeRegistryCommandlet : public UCommandlet
{
	GENERATED_BODY()
public:
	virtual int32 Main(const FString& Params) override;
};
