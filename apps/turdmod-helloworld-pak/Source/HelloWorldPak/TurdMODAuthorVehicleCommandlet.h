// TurdMODAuthorVehicleCommandlet — headless authoring of BP_TurdHeli, a CUSTOM
// VEHICLE Blueprint parented to SCUM's AAirplane (→ ADcxWheeledVehicle4W). Parenting
// to Airplane inherits the WHOLE mount system: F-to-enter, seats, replicated
// server-authoritative physics, AND native flight input — so the cooked vehicle is
// mountable + flyable with ZERO runtime code (the mech-derive pattern, but for a vehicle).
//
// ⚠ AAirplane is a SCUM game class, NOT compiled into this project. The commandlet
// resolves its UClass BY PATH at runtime (default /Script/SCUM.Airplane, override with
// -parent=<path>). For the EDITOR to author against it, SCUM's Airplane class must be
// loadable — i.e. a BORROWED class stub uasset in Content/SCUM/ (Dumper-7 / CUE4Parse)
// or the SCUM script module present. If it can't resolve, the commandlet fails LOUD
// (that resolution is the crux of the cook-against-native-class path).
//
// Run: UE4Editor-Cmd <uproject> -run=TurdMODAuthorVehicle -unattended -stdout
//      [ -parent=/Script/SCUM.Airplane ] [ -name=BP_TurdHeli ]
#pragma once

#include "CoreMinimal.h"
#include "Commandlets/Commandlet.h"
#include "TurdMODAuthorVehicleCommandlet.generated.h"

UCLASS()
class UTurdMODAuthorVehicleCommandlet : public UCommandlet
{
	GENERATED_BODY()

public:
	virtual int32 Main(const FString& Params) override;
};
