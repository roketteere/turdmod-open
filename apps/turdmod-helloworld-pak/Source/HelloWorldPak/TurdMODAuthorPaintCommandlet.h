// TurdMODAuthorPaintCommandlet — bake a custom material (M_TurdPaint) headlessly.
// Pure engine content (no SCUM parent needed), so it cooks clean. The loader applies
// it to a vehicle via SetMaterial — a clean engine call, unlike SCUM's gameplay paint
// RPC which is inert when called client-side. Proves the bake->apply path for vehicles.
// Run: UE4Editor-Cmd <uproject> -run=TurdMODAuthorPaint -unattended -stdout
#pragma once

#include "CoreMinimal.h"
#include "Commandlets/Commandlet.h"
#include "TurdMODAuthorPaintCommandlet.generated.h"

UCLASS()
class UTurdMODAuthorPaintCommandlet : public UCommandlet
{
	GENERATED_BODY()

public:
	virtual int32 Main(const FString& Params) override;
};
