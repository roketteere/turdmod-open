#pragma once

#include "CoreMinimal.h"
#include "GameFramework/Actor.h"
#include "TurdMODRouter.generated.h"

UCLASS()
class TURDMODLOADER_API ATurdMODRouter : public AActor
{
	GENERATED_BODY()

public:
	ATurdMODRouter();

	virtual void GetLifetimeReplicatedProps(
		TArray<FLifetimeProperty>& OutLifetimeProps) const override;

	UFUNCTION(NetMulticast, Reliable)
	void NetMulticast_HandleWidgetCommand(
		const FString& Command,
		const FString& WidgetClassPath,
		const FString& Payload);
};
