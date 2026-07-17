#include "TurdMODAuthorMechCommandlet.h"

#if WITH_EDITOR
#include "GameFramework/Character.h"
#include "Engine/Blueprint.h"
#include "Engine/BlueprintGeneratedClass.h"
#include "Kismet2/KismetEditorUtilities.h"
#include "UObject/Package.h"
#include "Misc/PackageName.h"
#endif

int32 UTurdMODAuthorMechCommandlet::Main(const FString& Params)
{
#if WITH_EDITOR
	const FString PkgName = TEXT("/Game/TurdMOD/BP_TurdMech");

	UPackage* Package = CreatePackage(*PkgName);
	if (!Package)
	{
		UE_LOG(LogTemp, Error, TEXT("[TurdMech] CreatePackage failed"));
		return 1;
	}
	Package->FullyLoad();

	// Clean ACharacter-parented Blueprint. ACharacter gives CapsuleComponent +
	// CharacterMovementComponent + a SkeletalMeshComponent slot — everything a
	// pilotable, walkable pawn needs — with ZERO combat AI. The runtime loader sets
	// the mesh/anim to a Sentry's and possesses it; no AI = no crash.
	UBlueprint* BP = FKismetEditorUtilities::CreateBlueprint(
		ACharacter::StaticClass(),
		Package,
		FName(TEXT("BP_TurdMech")),
		EBlueprintType::BPTYPE_Normal,
		UBlueprint::StaticClass(),
		UBlueprintGeneratedClass::StaticClass(),
		NAME_None);

	if (!BP)
	{
		UE_LOG(LogTemp, Error, TEXT("[TurdMech] CreateBlueprint failed"));
		return 2;
	}

	FKismetEditorUtilities::CompileBlueprint(BP);
	Package->MarkPackageDirty();

	const FString FileName = FPackageName::LongPackageNameToFilename(
		PkgName, FPackageName::GetAssetPackageExtension());
	const bool bSaved = UPackage::SavePackage(Package, BP, RF_Public | RF_Standalone, *FileName);

	UE_LOG(LogTemp, Warning, TEXT("[TurdMech] === BP_TurdMech authored: saved=%d  parent=ACharacter  genClass=%s ==="),
		bSaved ? 1 : 0, *GetNameSafe(BP->GeneratedClass));
	return bSaved ? 0 : 3;
#else
	UE_LOG(LogTemp, Error, TEXT("[TurdMech] requires WITH_EDITOR"));
	return 1;
#endif
}
