#include "TurdMODWidgetBase.h"

void UTurdMODWidgetBase::OnShowFromServer_Implementation(const FString& PayloadJson)
{
	FString TextValue;
	if (FParse::Value(*PayloadJson, TEXT("\"text\":\""), TextValue))
	{
		int32 QuoteIdx;
		if (TextValue.FindChar(TEXT('"'), QuoteIdx))
		{
			TextValue.LeftInline(QuoteIdx);
		}
		DisplayText = FText::FromString(TextValue);
	}

	FString DurationStr;
	if (FParse::Value(*PayloadJson, TEXT("\"duration\":"), DurationStr))
	{
		DurationStr.TrimStartAndEndInline();
		int32 TermIdx = INDEX_NONE;
		if (DurationStr.FindChar(TEXT(','), TermIdx) ||
			DurationStr.FindChar(TEXT('}'), TermIdx))
		{
			DurationStr.LeftInline(TermIdx);
		}
		AutoCloseDuration = FCString::Atof(*DurationStr);
	}

	if (AutoCloseDuration > 0.f)
	{
		UWorld* World = GetWorld();
		if (World)
		{
			FTimerHandle TimerHandle;
			FTimerDelegate TimerDelegate;
			TimerDelegate.BindWeakLambda(this, [this]()
			{
				RemoveFromParent();
			});
			World->GetTimerManager().SetTimer(
				TimerHandle, TimerDelegate, AutoCloseDuration, false);
		}
	}
}
