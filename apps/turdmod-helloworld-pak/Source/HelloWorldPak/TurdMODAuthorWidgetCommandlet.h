// TurdMODAuthorWidgetCommandlet — headless UMG authoring. UE4.27 Python can't
// build widget trees (no WidgetTree/WidgetBlueprintLibrary bindings), but C++ has
// the full API. This commandlet loads WBP_TurdMODWelcome and constructs its tree
// (panel + title + rules) entirely headlessly, then compiles + saves it.
// Run: UE4Editor-Cmd <uproject> -run=TurdMODAuthorWidget -unattended -stdout
// @ctx this is the real "cook & bake bridge" for visual-graph assets.
#pragma once

#include "CoreMinimal.h"
#include "Commandlets/Commandlet.h"
#include "TurdMODAuthorWidgetCommandlet.generated.h"

UCLASS()
class UTurdMODAuthorWidgetCommandlet : public UCommandlet
{
	GENERATED_BODY()

public:
	virtual int32 Main(const FString& Params) override;
};
