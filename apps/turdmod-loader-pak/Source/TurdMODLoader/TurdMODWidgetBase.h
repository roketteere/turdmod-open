#pragma once

#include "CoreMinimal.h"
#include "Blueprint/UserWidget.h"
#include "TurdMODWidgetBase.generated.h"

UCLASS(Blueprintable, BlueprintType)
class TURDMODLOADER_API UTurdMODWidgetBase : public UUserWidget
{
	GENERATED_BODY()

public:
	UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "TurdMOD")
	FText DisplayText;

	UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "TurdMOD")
	float AutoCloseDuration;

	UFUNCTION(BlueprintCallable, BlueprintNativeEvent, Category = "TurdMOD")
	void OnShowFromServer(const FString& PayloadJson);
};
