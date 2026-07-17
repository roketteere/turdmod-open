#include "TurdMODRouter.h"
#include "TurdMODWidgetBase.h"
#include "Net/UnrealNetwork.h"
#include "Blueprint/UserWidget.h"

ATurdMODRouter::ATurdMODRouter()
{
	bReplicates = true;
	bAlwaysRelevant = true;
	PrimaryActorTick.bCanEverTick = false;
}

void ATurdMODRouter::GetLifetimeReplicatedProps(
	TArray<FLifetimeProperty>& OutLifetimeProps) const
{
	Super::GetLifetimeReplicatedProps(OutLifetimeProps);
}

void ATurdMODRouter::NetMulticast_HandleWidgetCommand_Implementation(
	const FString& Command,
	const FString& WidgetClassPath,
	const FString& Payload)
{
	if (GetNetMode() == NM_DedicatedServer) return;

	UWorld* World = GetWorld();
	if (!World) return;
	APlayerController* PC = World->GetFirstPlayerController();
	if (!PC) return;

	if (Command == TEXT("Show"))
	{
		UClass* WidgetClass = StaticLoadClass(
			UUserWidget::StaticClass(), nullptr, *WidgetClassPath);
		if (!WidgetClass)
		{
			UE_LOG(LogTemp, Warning,
				TEXT("[TurdMODRouter] Show: failed to load '%s'"), *WidgetClassPath);
			return;
		}

		UUserWidget* Widget = CreateWidget<UUserWidget>(PC, WidgetClass);
		if (!Widget) return;

		UTurdMODWidgetBase* TurdWidget = Cast<UTurdMODWidgetBase>(Widget);
		if (TurdWidget)
		{
			TurdWidget->OnShowFromServer(Payload);
		}

		Widget->AddToViewport(100);
	}
	else if (Command == TEXT("Hide"))
	{
		UE_LOG(LogTemp, Log,
			TEXT("[TurdMODRouter] Hide: V2 — not yet implemented. Path='%s'"),
			*WidgetClassPath);
	}
	else if (Command == TEXT("Update"))
	{
		UE_LOG(LogTemp, Log,
			TEXT("[TurdMODRouter] Update: V2 — not yet implemented. Path='%s'"),
			*WidgetClassPath);
	}
}
