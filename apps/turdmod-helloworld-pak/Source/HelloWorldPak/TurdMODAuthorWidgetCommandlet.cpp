#include "TurdMODAuthorWidgetCommandlet.h"

#if WITH_EDITOR
#include "WidgetBlueprint.h"
#include "Blueprint/WidgetTree.h"
#include "Components/CanvasPanel.h"
#include "Components/CanvasPanelSlot.h"
#include "Components/Border.h"
#include "Components/VerticalBox.h"
#include "Components/VerticalBoxSlot.h"
#include "Components/TextBlock.h"
#include "Kismet2/KismetEditorUtilities.h"
#include "Kismet2/BlueprintEditorUtils.h"
#include "UObject/Package.h"
#include "Misc/PackageName.h"
#include "Layout/Margin.h"
#endif

int32 UTurdMODAuthorWidgetCommandlet::Main(const FString& Params)
{
#if WITH_EDITOR
	const FString ObjPath = TEXT("/Game/TurdMOD/WBP_TurdMODWelcome.WBP_TurdMODWelcome");

	UWidgetBlueprint* WBP = LoadObject<UWidgetBlueprint>(nullptr, *ObjPath);
	if (!WBP)
	{
		UE_LOG(LogTemp, Error, TEXT("[TurdMOD] WBP not found at %s (create the empty asset first)"), *ObjPath);
		return 1;
	}

	UWidgetTree* Tree = WBP->WidgetTree;
	if (!Tree)
	{
		Tree = NewObject<UWidgetTree>(WBP, UWidgetTree::StaticClass(), TEXT("WidgetTree"), RF_Transactional);
		WBP->WidgetTree = Tree;
	}

	// Rebuild the tree from scratch so re-runs are idempotent.
	Tree->RootWidget = nullptr;

	UCanvasPanel* Root = Tree->ConstructWidget<UCanvasPanel>(UCanvasPanel::StaticClass(), TEXT("RootCanvas"));
	Tree->RootWidget = Root;

	// Centered translucent panel (Rust-style welcome card).
	UBorder* Panel = Tree->ConstructWidget<UBorder>(UBorder::StaticClass(), TEXT("Panel"));
	Panel->SetBrushColor(FLinearColor(0.02f, 0.03f, 0.05f, 0.92f));
	Panel->SetPadding(FMargin(40.f, 32.f));
	UCanvasPanelSlot* PanelSlot = Root->AddChildToCanvas(Panel);
	PanelSlot->SetAnchors(FAnchors(0.5f, 0.5f));
	PanelSlot->SetAlignment(FVector2D(0.5f, 0.5f));
	PanelSlot->SetAutoSize(true);

	UVerticalBox* Stack = Tree->ConstructWidget<UVerticalBox>(UVerticalBox::StaticClass(), TEXT("Stack"));
	Panel->SetContent(Stack);

	auto AddLine = [&](const TCHAR* Name, const FString& Text, int32 Size, FLinearColor Color, float TopPad)
	{
		UTextBlock* T = Tree->ConstructWidget<UTextBlock>(UTextBlock::StaticClass(), Name);
		T->SetText(FText::FromString(Text));
		FSlateFontInfo Font = T->Font;
		Font.Size = Size;
		T->SetFont(Font);
		T->SetColorAndOpacity(FSlateColor(Color));
		UVerticalBoxSlot* S = Stack->AddChildToVerticalBox(T);
		S->SetPadding(FMargin(0.f, TopPad, 0.f, 0.f));
	};

	const FLinearColor Neon(0.0f, 0.83f, 1.0f, 1.0f);
	const FLinearColor Cream(0.92f, 0.90f, 0.85f, 1.0f);
	const FLinearColor Dim(0.74f, 0.74f, 0.78f, 1.0f);

	AddLine(TEXT("Title"), TEXT("TURDMOD"), 52, Neon, 0.f);
	AddLine(TEXT("Sub"), TEXT("Welcome, survivor."), 20, Cream, 6.f);
	AddLine(TEXT("RulesHdr"), TEXT("SERVER RULES"), 16, Neon, 22.f);
	AddLine(TEXT("R1"), TEXT("1.  Be excellent to each other."), 15, Dim, 10.f);
	AddLine(TEXT("R2"), TEXT("2.  BattlEye is OFF - play fair, no cheats."), 15, Dim, 6.f);
	AddLine(TEXT("R3"), TEXT("3.  PvP zones rotate - watch the map."), 15, Dim, 6.f);
	AddLine(TEXT("R4"), TEXT("4.  Vehicles & loot are boosted. Enjoy."), 15, Dim, 6.f);
	AddLine(TEXT("Foot"), TEXT("Powered by TurdMOD - report bugs to Fred."), 13, Neon, 22.f);

	// Compile the blueprint so the generated class carries the tree, then save.
	FBlueprintEditorUtils::MarkBlueprintAsStructurallyModified(WBP);
	FKismetEditorUtilities::CompileBlueprint(WBP);

	UPackage* Pkg = WBP->GetOutermost();
	Pkg->MarkPackageDirty();
	const FString FileName = FPackageName::LongPackageNameToFilename(
		Pkg->GetName(), FPackageName::GetAssetPackageExtension());
	const bool bSaved = UPackage::SavePackage(Pkg, WBP, RF_Public | RF_Standalone, *FileName);

	UE_LOG(LogTemp, Warning, TEXT("[TurdMOD] === WELCOME WIDGET AUTHORED: saved=%d  root=%s  children=%d ==="),
		bSaved ? 1 : 0, *Root->GetName(), Stack->GetChildrenCount());
	return bSaved ? 0 : 2;
#else
	UE_LOG(LogTemp, Error, TEXT("[TurdMOD] author commandlet requires WITH_EDITOR"));
	return 1;
#endif
}
