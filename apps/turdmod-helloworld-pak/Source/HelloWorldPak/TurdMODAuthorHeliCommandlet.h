// TurdMODAuthorHeliCommandlet — author a procedural helicopter STATIC MESH
// (SM_TurdHeli) + a simple material, headlessly. Proves the custom-MESH pipeline
// (vs the texture/material pipeline): build geometry in C++, cook it, pak it, and
// the loader spawns a StaticMeshActor with it so it renders in-world.
// Run: UE4Editor-Cmd <uproject> -run=TurdMODAuthorHeli -unattended -stdout
#pragma once

#include "CoreMinimal.h"
#include "Commandlets/Commandlet.h"
#include "TurdMODAuthorHeliCommandlet.generated.h"

UCLASS()
class UTurdMODAuthorHeliCommandlet : public UCommandlet
{
	GENERATED_BODY()

public:
	virtual int32 Main(const FString& Params) override;
};
