// TurdMODAuthorPresetCommandlet — headless authoring of VP_TurdJeep, a UVehiclePreset
// data asset whose VehicleClass points at our BP_TurdJeep. Tests whether SCUM's native
// VehicleManager spawns preset.VehicleClass (vs the map-key class). RootNode is left null
// at author time — wired at runtime via the bridge to a working stock parts-tree.
//
// Run: UE4Editor-Cmd <uproject> -run=TurdMODAuthorPreset -unattended -nullrhi
//      [ -name=VP_TurdJeep ] [ -path=/Game/ConZ_Files/Vehicles/TurdMOD ]
//      [ -vehicle=/Game/ConZ_Files/Vehicles/TurdMOD/BP_TurdJeep.BP_TurdJeep_C ]
#pragma once

#include "CoreMinimal.h"
#include "Commandlets/Commandlet.h"
#include "TurdMODAuthorPresetCommandlet.generated.h"

UCLASS()
class UTurdMODAuthorPresetCommandlet : public UCommandlet
{
	GENERATED_BODY()
public:
	virtual int32 Main(const FString& Params) override;
};
