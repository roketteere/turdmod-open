// TurdMODAuthorMechCommandlet — headless authoring of BP_TurdMech, a CLEAN
// pilotable mech pawn: a Blueprint parented to engine ACharacter (CharacterMovement
// + capsule + mesh slot for free) with NO combat AI (the thing that crashed live
// Sentries). Runtime sets its mesh/anim to a Sentry's and possesses it.
// Run: UE4Editor-Cmd <uproject> -run=TurdMODAuthorMech -unattended -stdout
#pragma once

#include "CoreMinimal.h"
#include "Commandlets/Commandlet.h"
#include "TurdMODAuthorMechCommandlet.generated.h"

UCLASS()
class UTurdMODAuthorMechCommandlet : public UCommandlet
{
	GENERATED_BODY()

public:
	virtual int32 Main(const FString& Params) override;
};
