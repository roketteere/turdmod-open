#include "TurdMODAuthorPaintCommandlet.h"

#if WITH_EDITOR
#include "Engine/Texture2D.h"
#include "Materials/Material.h"
#include "Materials/MaterialExpressionTextureSample.h"
#include "Materials/MaterialExpressionWorldPosition.h"
#include "Materials/MaterialExpressionActorPositionWS.h"
#include "Materials/MaterialExpressionSubtract.h"
#include "Materials/MaterialExpressionMultiply.h"
#include "Materials/MaterialExpressionComponentMask.h"
#include "Materials/MaterialExpressionAdd.h"
#include "Materials/MaterialExpressionConstant.h"
#include "Materials/MaterialExpressionConstant3Vector.h"
#include "Materials/MaterialExpressionDotProduct.h"
#include "Materials/MaterialExpressionAppendVector.h"
#include "Materials/MaterialExpressionCollectionParameter.h"
#include "Materials/MaterialParameterCollection.h"
#include "AssetToolsModule.h"
#include "IAssetTools.h"
#include "AssetImportTask.h"
#include "Factories/TextureFactory.h"
#include "Engine/StaticMeshActor.h"
#include "Engine/StaticMesh.h"
#include "Engine/World.h"
#include "Engine/Engine.h"
#include "AssetRegistryModule.h"
#include "UObject/Package.h"
#include "Misc/PackageName.h"
#endif

#if WITH_EDITOR
// Import a PNG on disk as a saved UTexture2D asset at /Game/TurdMOD/<Name>.
static UTexture2D* ImportTexture(const FString& PngPath, const FString& Name, bool bSRGB = true)
{
	FAssetToolsModule& AssetToolsModule =
		FModuleManager::LoadModuleChecked<FAssetToolsModule>("AssetTools");

	UAssetImportTask* Task = NewObject<UAssetImportTask>();
	Task->Filename = PngPath;
	Task->DestinationPath = TEXT("/Game/TurdMOD");
	Task->DestinationName = Name;
	Task->bAutomated = true;
	Task->bSave = true;
	Task->bReplaceExisting = true;
	UTextureFactory* Factory = NewObject<UTextureFactory>();
	Factory->bCreateMaterial = false;
	Task->Factory = Factory;

	TArray<UAssetImportTask*> Tasks;
	Tasks.Add(Task);
	AssetToolsModule.Get().ImportAssetTasks(Tasks);

	// Load the imported texture by its known destination path.
	const FString ObjPath = FString::Printf(TEXT("/Game/TurdMOD/%s.%s"), *Name, *Name);
	UTexture2D* Tex = LoadObject<UTexture2D>(nullptr, *ObjPath);
	// @inv RMA / data textures MUST be LINEAR — sampling rough/metal through sRGB
	// gamma-distorts the values. Force sRGB off + Masks compression and re-save.
	if (Tex && !bSRGB)
	{
		Tex->SRGB = false;
		Tex->CompressionSettings = TextureCompressionSettings::TC_Masks;
		Tex->PostEditChange();
		Tex->UpdateResource();
		if (UPackage* Pkg = Tex->GetOutermost())
		{
			Pkg->MarkPackageDirty();
			const FString Fn = FPackageName::LongPackageNameToFilename(Pkg->GetName(), FPackageName::GetAssetPackageExtension());
			UPackage::SavePackage(Pkg, Tex, RF_Public | RF_Standalone, *Fn);
		}
	}
	return Tex;
}

// Bake a material at /Game/TurdMOD/<Name> whose BaseColor (and Emissive, for punch)
// samples `Tex`. TextureSample outputs vec3 → matches the inputs (no grey-default).
static bool BakeTextureMaterial(const FString& Name, UTexture2D* Tex, UTexture2D* RmaTex = nullptr)
{
	if (!Tex) { return false; }
	const FString PkgName = FString::Printf(TEXT("/Game/TurdMOD/%s"), *Name);
	UPackage* Package = CreatePackage(*PkgName);
	if (!Package) { return false; }
	Package->FullyLoad();

	UMaterial* Mat = NewObject<UMaterial>(Package, FName(*Name), RF_Public | RF_Standalone);
	UMaterialExpressionTextureSample* TS = NewObject<UMaterialExpressionTextureSample>(Mat);
	TS->Texture = Tex;
	TS->SamplerType = SAMPLERTYPE_Color;
	TS->MaterialExpressionEditorX = -360;
	Mat->Expressions.Add(TS);
	// @inv LIT (default shading), BaseColor only, NO emissive. The livery is lit by
	// the world like stock paint — dark at night, no glow. @brk adding EmissiveColor
	// or MSM_Unlit makes it self-illuminate and give the player's position away at
	// night (Joel, 2026-06-15).
	Mat->BaseColor.Expression = TS;
	// Optional finish: sample the RMA skin and wire Roughness(R)/Metallic(G). Packing
	// matches SCUM's "Rough/Metal/AO". @inv RMA sampled LINEAR (SAMPLERTYPE_LinearColor)
	// + imported with SRGB off, or the finish values gamma-distort. TextureSample output
	// indices: 1=R, 2=G. Left at material defaults when no RMA provided.
	if (RmaTex)
	{
		UMaterialExpressionTextureSample* RTS = NewObject<UMaterialExpressionTextureSample>(Mat);
		RTS->Texture = RmaTex;
		RTS->SamplerType = SAMPLERTYPE_Masks;   // @inv must match the RMA's TC_Masks import, or the material fails to compile (grey husk)
		RTS->MaterialExpressionEditorX = -360;
		RTS->MaterialExpressionEditorY = 260;
		Mat->Expressions.Add(RTS);
		Mat->Roughness.Expression = RTS; Mat->Roughness.OutputIndex = 1;  // R
		Mat->Metallic.Expression  = RTS; Mat->Metallic.OutputIndex  = 2;  // G
	}
	// TwoSided so thin body shells show the livery on both faces.
	Mat->TwoSided = true;

	// @inv vehicle parts are a MIX of static / instanced-static / skeletal / spline
	// mesh components. Each vertex-factory type needs its OWN compiled shader
	// permutation; without these usage flags the cooker only builds the base
	// static-mesh permutation and every other mesh type renders default-GREY.
	// Setting the flags forces all permutations to compile INLINE at cook.
	// @brk drop a flag and the matching mesh type goes grey again.
	Mat->bUsedWithSkeletalMesh = true;
	Mat->bUsedWithInstancedStaticMeshes = true;
	Mat->bUsedWithStaticLighting = true;
	Mat->bUsedWithSplineMeshes = true;
	Mat->bUsedWithMorphTargets = true;

	Mat->PreEditChange(nullptr);
	Mat->PostEditChange();

	Package->MarkPackageDirty();
	const FString FileName = FPackageName::LongPackageNameToFilename(
		PkgName, FPackageName::GetAssetPackageExtension());
	const bool bSaved = UPackage::SavePackage(Package, Mat, RF_Public | RF_Standalone, *FileName);
	UE_LOG(LogTemp, Warning, TEXT("[TurdPaint] %s baked: saved=%d"), *Name, bSaved ? 1 : 0);
	return bSaved;
}

// Bake an UNLIT material that projects `Tex` TOP-DOWN over the whole vehicle,
// centered on the owning actor. UV = (WorldPos - ActorPos) * Scale + 0.5, taking
// the world X/Y. @inv ActorPositionWS returns the OWNING ACTOR's origin, which is
// identical for every body-part component on one vehicle — so the flag reads as a
// single coherent image draped across the car instead of per-panel fragments.
// Uniform Scale keeps the projection heading-agnostic (no per-axis stretch as the
// car turns). Scale ~0.002 => a 500cm square centered on the car maps to the full
// flag (covers a Laika). @brk per-axis scale would stretch differently per heading.
static bool BakeProjectedMaterial(const FString& Name, UTexture2D* Tex, float Scale)
{
	if (!Tex) { return false; }
	const FString PkgName = FString::Printf(TEXT("/Game/TurdMOD/%s"), *Name);
	UPackage* Package = CreatePackage(*PkgName);
	if (!Package) { return false; }
	Package->FullyLoad();

	UMaterial* Mat = NewObject<UMaterial>(Package, FName(*Name), RF_Public | RF_Standalone);

	UMaterialExpressionWorldPosition* WP = NewObject<UMaterialExpressionWorldPosition>(Mat);
	// ActorPositionWS lacks the ENGINE_API export macro in 4.27, so its static
	// class symbol won't link — construct it via its registered UClass by name.
	UClass* ApCls = FindObject<UClass>(ANY_PACKAGE, TEXT("MaterialExpressionActorPositionWS"));
	UMaterialExpression* AP = ApCls ? NewObject<UMaterialExpression>(Mat, ApCls) : nullptr;
	if (!AP) { UE_LOG(LogTemp, Error, TEXT("[TurdPaint] ActorPositionWS class not found")); return false; }
	UMaterialExpressionSubtract* Sub = NewObject<UMaterialExpressionSubtract>(Mat);
	Sub->A.Expression = WP; Sub->B.Expression = AP;            // world offset from car center
	UMaterialExpressionConstant3Vector* Sc = NewObject<UMaterialExpressionConstant3Vector>(Mat);
	Sc->Constant = FLinearColor(Scale, Scale, 0.f, 0.f);
	UMaterialExpressionMultiply* Mul = NewObject<UMaterialExpressionMultiply>(Mat);
	Mul->A.Expression = Sub; Mul->B.Expression = Sc;
	UMaterialExpressionComponentMask* Mask = NewObject<UMaterialExpressionComponentMask>(Mat);
	Mask->Input.Expression = Mul; Mask->R = true; Mask->G = true; Mask->B = false; Mask->A = false;
	UMaterialExpressionConstant* Half = NewObject<UMaterialExpressionConstant>(Mat);
	Half->R = 0.5f;                                            // recenter 0..1
	UMaterialExpressionAdd* Add = NewObject<UMaterialExpressionAdd>(Mat);
	Add->A.Expression = Mask; Add->B.Expression = Half;        // scalar broadcasts to float2
	UMaterialExpressionTextureSample* TS = NewObject<UMaterialExpressionTextureSample>(Mat);
	TS->Texture = Tex; TS->SamplerType = SAMPLERTYPE_Color; TS->Coordinates.Expression = Add;

	Mat->Expressions.Add(WP);  Mat->Expressions.Add(AP);  Mat->Expressions.Add(Sub);
	Mat->Expressions.Add(Sc);  Mat->Expressions.Add(Mul); Mat->Expressions.Add(Mask);
	Mat->Expressions.Add(Half);Mat->Expressions.Add(Add); Mat->Expressions.Add(TS);

	Mat->BaseColor.Expression = TS;
	Mat->EmissiveColor.Expression = TS;
	Mat->SetShadingModel(MSM_Unlit);   // flat, full-bright from every angle
	Mat->TwoSided = true;
	Mat->bUsedWithSkeletalMesh = true;
	Mat->bUsedWithInstancedStaticMeshes = true;
	Mat->bUsedWithStaticLighting = true;
	Mat->bUsedWithSplineMeshes = true;
	Mat->bUsedWithMorphTargets = true;

	Mat->PreEditChange(nullptr);
	Mat->PostEditChange();
	Package->MarkPackageDirty();
	const FString FileName = FPackageName::LongPackageNameToFilename(
		PkgName, FPackageName::GetAssetPackageExtension());
	const bool bSaved = UPackage::SavePackage(Package, Mat, RF_Public | RF_Standalone, *FileName);
	UE_LOG(LogTemp, Warning, TEXT("[TurdPaint] %s (projected) baked: saved=%d"), *Name, bSaved ? 1 : 0);
	return bSaved;
}

// Create a Material Parameter Collection the LOADER updates every tick with the
// car's frame: CarCenter (world origin), CarX/CarY (the car's right/forward axes,
// PRE-SCALED so the dot product lands in 0..1 over the car footprint). The
// car-local projection material reads these → flag stays fixed to the body and
// rotates WITH the car (no world-axis swim).
static UMaterialParameterCollection* CreateCarFrameMPC()
{
	const FString PkgName = TEXT("/Game/TurdMOD/MPC_CarFrame");
	UPackage* Pkg = CreatePackage(*PkgName);
	if (!Pkg) { return nullptr; }
	Pkg->FullyLoad();
	UMaterialParameterCollection* MPC =
		NewObject<UMaterialParameterCollection>(Pkg, FName(TEXT("MPC_CarFrame")), RF_Public | RF_Standalone);
	const TCHAR* Names[] = { TEXT("CarCenter"), TEXT("CarX"), TEXT("CarY") };
	for (const TCHAR* N : Names)
	{
		FCollectionVectorParameter P;
		P.Id = FGuid::NewGuid();
		P.ParameterName = FName(N);
		P.DefaultValue = FLinearColor(0.f, 0.f, 0.f, 0.f);
		MPC->VectorParameters.Add(P);
	}
	MPC->PostEditChange();
	Pkg->MarkPackageDirty();
	const FString FileName = FPackageName::LongPackageNameToFilename(
		PkgName, FPackageName::GetAssetPackageExtension());
	const bool bSaved = UPackage::SavePackage(Pkg, MPC, RF_Public | RF_Standalone, *FileName);
	UE_LOG(LogTemp, Warning, TEXT("[TurdPaint] MPC_CarFrame saved=%d params=%d"),
		bSaved ? 1 : 0, MPC->VectorParameters.Num());
	return MPC;
}

// Bake the CAR-LOCAL projected flag (textured, Unlit). UV = ( dot(WP-Center, CarX),
// dot(WP-Center, CarY) ) + 0.5 → samples T_PRFlag in the car's own frame. Coherent
// flag draped over the whole body, oriented to the car. @dep MPC_CarFrame fed live
// by the loader tick.
static bool BakeCarLocalFlag(const FString& Name, UTexture2D* Tex, UMaterialParameterCollection* MPC)
{
	if (!Tex || !MPC) { return false; }
	const FString PkgName = FString::Printf(TEXT("/Game/TurdMOD/%s"), *Name);
	UPackage* Package = CreatePackage(*PkgName);
	if (!Package) { return false; }
	Package->FullyLoad();
	UMaterial* Mat = NewObject<UMaterial>(Package, FName(*Name), RF_Public | RF_Standalone);

	// collection param -> masked to RGB (vec3) so dot products match WorldPosition.
	auto Coll3 = [&](const TCHAR* PN) -> UMaterialExpression*
	{
		auto* C = NewObject<UMaterialExpressionCollectionParameter>(Mat);
		C->Collection = MPC;
		C->ParameterName = FName(PN);
		C->ParameterId = MPC->GetParameterId(FName(PN));
		Mat->Expressions.Add(C);
		auto* M = NewObject<UMaterialExpressionComponentMask>(Mat);
		M->Input.Expression = C; M->R = true; M->G = true; M->B = true; M->A = false;
		Mat->Expressions.Add(M);
		return M;
	};

	auto* WP = NewObject<UMaterialExpressionWorldPosition>(Mat); Mat->Expressions.Add(WP);
	UMaterialExpression* Center = Coll3(TEXT("CarCenter"));
	UMaterialExpression* CarX = Coll3(TEXT("CarX"));
	UMaterialExpression* CarY = Coll3(TEXT("CarY"));
	auto* Sub = NewObject<UMaterialExpressionSubtract>(Mat); Sub->A.Expression = WP; Sub->B.Expression = Center; Mat->Expressions.Add(Sub);
	auto* DotX = NewObject<UMaterialExpressionDotProduct>(Mat); DotX->A.Expression = Sub; DotX->B.Expression = CarX; Mat->Expressions.Add(DotX);
	auto* DotY = NewObject<UMaterialExpressionDotProduct>(Mat); DotY->A.Expression = Sub; DotY->B.Expression = CarY; Mat->Expressions.Add(DotY);
	auto* Half = NewObject<UMaterialExpressionConstant>(Mat); Half->R = 0.5f; Mat->Expressions.Add(Half);
	auto* AddX = NewObject<UMaterialExpressionAdd>(Mat); AddX->A.Expression = DotX; AddX->B.Expression = Half; Mat->Expressions.Add(AddX);
	auto* AddY = NewObject<UMaterialExpressionAdd>(Mat); AddY->A.Expression = DotY; AddY->B.Expression = Half; Mat->Expressions.Add(AddY);
	auto* UV = NewObject<UMaterialExpressionAppendVector>(Mat); UV->A.Expression = AddX; UV->B.Expression = AddY; Mat->Expressions.Add(UV);
	auto* TS = NewObject<UMaterialExpressionTextureSample>(Mat); TS->Texture = Tex; TS->SamplerType = SAMPLERTYPE_Color; TS->Coordinates.Expression = UV; Mat->Expressions.Add(TS);

	Mat->BaseColor.Expression = TS;
	Mat->EmissiveColor.Expression = TS;
	Mat->SetShadingModel(MSM_Unlit);
	Mat->TwoSided = true;
	Mat->bUsedWithSkeletalMesh = true;
	Mat->bUsedWithInstancedStaticMeshes = true;
	Mat->bUsedWithStaticLighting = true;
	Mat->bUsedWithSplineMeshes = true;
	Mat->bUsedWithMorphTargets = true;

	Mat->PreEditChange(nullptr);
	Mat->PostEditChange();
	Package->MarkPackageDirty();
	const FString FileName = FPackageName::LongPackageNameToFilename(
		PkgName, FPackageName::GetAssetPackageExtension());
	const bool bSaved = UPackage::SavePackage(Package, Mat, RF_Public | RF_Standalone, *FileName);
	UE_LOG(LogTemp, Warning, TEXT("[TurdPaint] %s (car-local) baked: saved=%d"), *Name, bSaved ? 1 : 0);
	return bSaved;
}

// Bake a TRANSLUCENT unlit tint material — a dark wash at `Opacity` that darkens
// what's behind it without blocking the view (window tint you can still see out of
// in first person). @inv BLEND_Translucent + low Opacity => see-through.
static bool BakeTintMaterial(const FString& Name, FLinearColor Color, float Opacity)
{
	const FString PkgName = FString::Printf(TEXT("/Game/TurdMOD/%s"), *Name);
	UPackage* Package = CreatePackage(*PkgName);
	if (!Package) { return false; }
	Package->FullyLoad();

	UMaterial* Mat = NewObject<UMaterial>(Package, FName(*Name), RF_Public | RF_Standalone);
	UMaterialExpressionConstant3Vector* C = NewObject<UMaterialExpressionConstant3Vector>(Mat);
	C->Constant = Color;
	UMaterialExpressionConstant* O = NewObject<UMaterialExpressionConstant>(Mat);
	O->R = Opacity;
	Mat->Expressions.Add(C); Mat->Expressions.Add(O);
	Mat->EmissiveColor.Expression = C;
	Mat->Opacity.Expression = O;
	Mat->BlendMode = BLEND_Translucent;
	Mat->SetShadingModel(MSM_Unlit);
	Mat->TwoSided = true;
	Mat->bUsedWithSkeletalMesh = true;
	Mat->bUsedWithInstancedStaticMeshes = true;
	Mat->bUsedWithStaticLighting = true;
	Mat->bUsedWithSplineMeshes = true;

	Mat->PreEditChange(nullptr);
	Mat->PostEditChange();
	Package->MarkPackageDirty();
	const FString FileName = FPackageName::LongPackageNameToFilename(
		PkgName, FPackageName::GetAssetPackageExtension());
	const bool bSaved = UPackage::SavePackage(Package, Mat, RF_Public | RF_Standalone, *FileName);
	UE_LOG(LogTemp, Warning, TEXT("[TurdPaint] %s (tint) baked: saved=%d"), *Name, bSaved ? 1 : 0);
	return bSaved;
}

// Create + save a throwaway map placing cube meshes that USE the materials. Cooking this
// map forces the cooker to compile each material's static-mesh shader permutations into
// the material packages — without this a runtime-applied material renders default grey.
static bool CreateLiveryMap(const TArray<UMaterialInterface*>& Mats)
{
	const FString MapPkgName = TEXT("/Game/TurdMOD/M_LiveryCook");
	UPackage* Pkg = CreatePackage(*MapPkgName);
	if (!Pkg) { return false; }
	Pkg->SetPackageFlags(PKG_ContainsMap);

	UWorld* World = NewObject<UWorld>(Pkg, FName(TEXT("M_LiveryCook")), RF_Public | RF_Standalone);
	World->WorldType = EWorldType::Inactive;
	World->InitializeNewWorld(UWorld::InitializationValues()
		.CreatePhysicsScene(false).ShouldSimulatePhysics(false)
		.EnableTraceCollision(false).CreateNavigation(false).CreateAISystem(false));

	UStaticMesh* Cube = LoadObject<UStaticMesh>(nullptr, TEXT("/Engine/BasicShapes/Cube.Cube"));
	int32 Placed = 0;
	if (Cube)
	{
		for (UMaterialInterface* M : Mats)
		{
			if (!M) { continue; }
			AStaticMeshActor* A = World->SpawnActor<AStaticMeshActor>();
			if (A && A->GetStaticMeshComponent())
			{
				A->GetStaticMeshComponent()->SetStaticMesh(Cube);
				A->GetStaticMeshComponent()->SetMaterial(0, M);
				Placed++;
			}
		}
	}
	FAssetRegistryModule::AssetCreated(World);
	Pkg->MarkPackageDirty();
	const FString FileName = FPackageName::LongPackageNameToFilename(
		MapPkgName, FPackageName::GetMapPackageExtension());
	const bool bSaved = UPackage::SavePackage(Pkg, World, RF_Public | RF_Standalone, *FileName);
	UE_LOG(LogTemp, Warning, TEXT("[TurdPaint] livery map: cube=%d placed=%d saved=%d"),
		Cube ? 1 : 0, Placed, bSaved ? 1 : 0);
	return bSaved;
}
#endif

int32 UTurdMODAuthorPaintCommandlet::Main(const FString& Params)
{
#if WITH_EDITOR
	const FString FlagPng = TEXT("C:/Development/Claude/turdmod/brand/pr-flag.png");
	const FString BodyPng = TEXT("C:/Development/Claude/turdmod/brand/pr-flag-body.png");

	// T_PRFlag = plain flag (wheels, per-UV). T_PRFlagBody = the flag BAKED into the
	// Laika body UV atlas (3D-projected via the extractor) — sampling it per-UV draws
	// a coherent flag draped over the body, no swim, no runtime feed.
	UTexture2D* FlagTex = ImportTexture(FlagPng, TEXT("T_PRFlag"));
	UTexture2D* BodyTex = ImportTexture(BodyPng, TEXT("T_PRFlagBody"));
	UE_LOG(LogTemp, Warning, TEXT("[TurdPaint] textures: flag=%d body=%d"),
		FlagTex ? 1 : 0, BodyTex ? 1 : 0);

	bool ok1 = BakeTextureMaterial(TEXT("M_PRFlag"), FlagTex);       // wheels (legacy whole-wheel flag)
	bool okB = BakeTextureMaterial(TEXT("M_PRFlagBody"), BodyTex);   // body (baked atlas)
	UE_LOG(LogTemp, Warning, TEXT("[TurdPaint] body livery baked: %d"), okB ? 1 : 0);

	// Wheel skin: the liverylab wheel-skin bake (rim/tire painted into the real
	// T_Laika_Wheels_D UV space) -> a material we SetMaterial onto the wheel, exactly
	// like M_PRFlag. Optional: only baked if the studio exported a skin.
	const FString WheelPng = TEXT("C:/Development/Claude/turdmod/brand/laika-wheelskin.png");
	UMaterialInterface* MWheel = nullptr;
	if (UTexture2D* WheelTex = ImportTexture(WheelPng, TEXT("T_LaikaWheelSkin")))   // null if the studio hasn't exported a skin
	{
		UTexture2D* WheelRma = ImportTexture(TEXT("C:/Development/Claude/turdmod/brand/laika-wheelskin-rma.png"), TEXT("T_LaikaWheelSkin_RMA"), false);  // finish (rough/metal); null ok
		bool okW = BakeTextureMaterial(TEXT("M_LaikaWheelSkin"), WheelTex, WheelRma);
		UE_LOG(LogTemp, Warning, TEXT("[TurdPaint] wheel skin baked: %d (rma=%d)"), okW ? 1 : 0, WheelRma ? 1 : 0);
		if (okW) MWheel = LoadObject<UMaterial>(nullptr, TEXT("/Game/TurdMOD/M_LaikaWheelSkin.M_LaikaWheelSkin"));
	}
	else { UE_LOG(LogTemp, Warning, TEXT("[TurdPaint] no wheel skin texture imported (skipping)")); }

	// Body skin: the liverylab body bake (per-part paint in the real T_Laika_Metal_D
	// UV space) -> M_LaikaBodySkin, SetMaterial'd onto MI_Laika_Outside_A like M_PRFlagBody.
	const FString BodySkinPng = TEXT("C:/Development/Claude/turdmod/brand/laika-bodyskin.png");
	UMaterialInterface* MBody = nullptr;
	if (UTexture2D* BodySkinTex = ImportTexture(BodySkinPng, TEXT("T_LaikaBodySkin")))
	{
		UTexture2D* BodyRma = ImportTexture(TEXT("C:/Development/Claude/turdmod/brand/laika-bodyskin-rma.png"), TEXT("T_LaikaBodySkin_RMA"), false);  // finish (rough/metal); null ok
		bool okBS = BakeTextureMaterial(TEXT("M_LaikaBodySkin"), BodySkinTex, BodyRma);
		UE_LOG(LogTemp, Warning, TEXT("[TurdPaint] body skin baked: %d (rma=%d)"), okBS ? 1 : 0, BodyRma ? 1 : 0);
		if (okBS) MBody = LoadObject<UMaterial>(nullptr, TEXT("/Game/TurdMOD/M_LaikaBodySkin.M_LaikaBodySkin"));
	}
	else { UE_LOG(LogTemp, Warning, TEXT("[TurdPaint] no body skin texture imported (skipping)")); }

	// reference materials from a cooked map so their shaders compile inline.
	UMaterialInterface* M1 = LoadObject<UMaterial>(nullptr, TEXT("/Game/TurdMOD/M_PRFlag.M_PRFlag"));
	UMaterialInterface* M3 = LoadObject<UMaterial>(nullptr, TEXT("/Game/TurdMOD/M_PRFlagBody.M_PRFlagBody"));
	TArray<UMaterialInterface*> Mats; Mats.Add(M1); Mats.Add(M3);
	if (MWheel) Mats.Add(MWheel);
	if (MBody) Mats.Add(MBody);
	bool ok3 = CreateLiveryMap(Mats);

	UE_LOG(LogTemp, Warning, TEXT("[TurdPaint] === DONE: M_PRFlag=%d body=%d wheel=%d bodyskin=%d map=%d ==="), ok1, okB, MWheel ? 1 : 0, MBody ? 1 : 0, ok3);
	return (ok1 && okB && ok3) ? 0 : 3;
#else
	UE_LOG(LogTemp, Error, TEXT("[TurdPaint] requires WITH_EDITOR"));
	return 1;
#endif
}
