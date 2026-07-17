#include "TurdMODAuthorHeliCommandlet.h"

#include "Engine/StaticMesh.h"
#include "Materials/Material.h"
#include "Materials/MaterialExpressionConstant3Vector.h"
#include "Materials/MaterialExpressionConstant.h"
#include "RawMesh.h"
#include "UObject/Package.h"
#include "Misc/PackageName.h"
#include "Misc/FileHelper.h"
#include "Misc/Paths.h"
#include "AssetRegistry/AssetRegistryModule.h"
#include "Engine/StaticMeshActor.h"
#include "Components/StaticMeshComponent.h"
#include "Engine/World.h"

// Load a real model exported by glb2mesh.mjs (format: "V n\n x y z*n\n F m\n i i i*m").
// Returns true and fills Raw if the file exists + parses. UVs are placeholder (single
// color material); normals are recomputed at build.
static bool ReadMeshFile(const FString& Path, FRawMesh& Raw)
{
	FString Text;
	if (!FFileHelper::LoadFileToString(Text, *Path)) return false;
	TArray<FString> Lines; Text.ParseIntoArrayLines(Lines);
	int32 li = 0, nv = 0, nf = 0;
	auto NextTok = [](const FString& L, int& cur, FString& tok){ /*unused*/ };
	for (; li < Lines.Num(); ++li) { if (Lines[li].StartsWith(TEXT("V "))) { nv = FCString::Atoi(*Lines[li].Mid(2)); ++li; break; } }
	for (int i = 0; i < nv && li < Lines.Num(); ++i, ++li)
	{
		TArray<FString> p; Lines[li].ParseIntoArray(p, TEXT(" "), true);
		if (p.Num() >= 3) Raw.VertexPositions.Add(FVector(FCString::Atof(*p[0]), FCString::Atof(*p[1]), FCString::Atof(*p[2])));
	}
	for (; li < Lines.Num(); ++li) { if (Lines[li].StartsWith(TEXT("F "))) { nf = FCString::Atoi(*Lines[li].Mid(2)); ++li; break; } }
	static const FVector2D uvq[3] = { {0,0},{1,0},{1,1} };
	for (int i = 0; i < nf && li < Lines.Num(); ++i, ++li)
	{
		TArray<FString> p; Lines[li].ParseIntoArray(p, TEXT(" "), true);
		if (p.Num() < 3) continue;
		for (int k = 0; k < 3; ++k) { Raw.WedgeIndices.Add(FCString::Atoi(*p[k])); Raw.WedgeTexCoords[0].Add(uvq[k]); }
		Raw.FaceMaterialIndices.Add(0); Raw.FaceSmoothingMasks.Add(1);
	}
	return Raw.VertexPositions.Num() > 0 && Raw.WedgeIndices.Num() >= 3;
}

// Append an axis-aligned box (Mn..Mx) to the raw mesh. Material is TwoSided so we
// don't fuss over per-face winding; normals are recomputed at build.
static void AddBox(FRawMesh& M, const FVector& Mn, const FVector& Mx)
{
	const int32 base = M.VertexPositions.Num();
	const FVector c[8] = {
		{Mn.X,Mn.Y,Mn.Z},{Mx.X,Mn.Y,Mn.Z},{Mx.X,Mx.Y,Mn.Z},{Mn.X,Mx.Y,Mn.Z},
		{Mn.X,Mn.Y,Mx.Z},{Mx.X,Mn.Y,Mx.Z},{Mx.X,Mx.Y,Mx.Z},{Mn.X,Mx.Y,Mx.Z}
	};
	for (int i = 0; i < 8; ++i) M.VertexPositions.Add(c[i]);

	static const int32 quads[6][4] = {
		{0,1,2,3},{4,7,6,5},{0,4,5,1},{1,5,6,2},{2,6,7,3},{3,7,4,0}
	};
	static const FVector2D uvq[4] = { {0,0},{1,0},{1,1},{0,1} };
	for (int f = 0; f < 6; ++f)
	{
		const int32* q = quads[f];
		const int32 tri[6]   = { q[0],q[1],q[2], q[0],q[2],q[3] };
		const int32 uvidx[6] = { 0,1,2, 0,2,3 };
		for (int k = 0; k < 6; ++k)
		{
			M.WedgeIndices.Add(base + tri[k]);
			M.WedgeTexCoords[0].Add(uvq[uvidx[k]]);
		}
		M.FaceMaterialIndices.Add(0); M.FaceMaterialIndices.Add(0);
		M.FaceSmoothingMasks.Add(0);  M.FaceSmoothingMasks.Add(0);
	}
}

// Simple lit constant-color material (with the usage flags so it cooks for static
// meshes; inline shaders come from the project's bShareMaterialShaderCode=False).
static UMaterial* MakeColorMaterial(const FString& Name, FLinearColor Color)
{
	const FString PkgName = FString::Printf(TEXT("/Game/TurdMOD/%s"), *Name);
	UPackage* Pkg = CreatePackage(*PkgName);
	Pkg->FullyLoad();
	UMaterial* Mat = NewObject<UMaterial>(Pkg, FName(*Name), RF_Public | RF_Standalone);
	UMaterialExpressionConstant3Vector* C = NewObject<UMaterialExpressionConstant3Vector>(Mat);
	C->Constant = Color;
	Mat->Expressions.Add(C);
	Mat->BaseColor.Expression = C;
	Mat->TwoSided = true;
	Mat->bUsedWithStaticLighting = true;
	Mat->bUsedWithInstancedStaticMeshes = true;
	Mat->PreEditChange(nullptr);
	Mat->PostEditChange();
	Pkg->MarkPackageDirty();
	const FString Fn = FPackageName::LongPackageNameToFilename(PkgName, FPackageName::GetAssetPackageExtension());
	UPackage::SavePackage(Pkg, Mat, RF_Public | RF_Standalone, *Fn);
	return Mat;
}

int32 UTurdMODAuthorHeliCommandlet::Main(const FString& /*Params*/)
{
#if WITH_EDITOR
	UMaterial* Mat = MakeColorMaterial(TEXT("M_TurdHeli"), FLinearColor(0.16f, 0.20f, 0.14f)); // olive

	FRawMesh Raw;
	// Prefer a REAL model exported to brand/heli.mesh (glb2mesh.mjs from a CC-BY GLB,
	// "Poly by Google"); fall back to the procedural box heli if the file is absent.
	const FString MeshFile = TEXT("C:/Development/Claude/turdmod/brand/heli.mesh");
	const bool bReal = ReadMeshFile(MeshFile, Raw);
	UE_LOG(LogTemp, Warning, TEXT("[TurdHeli] model source: %s"), bReal ? TEXT("brand/heli.mesh (real)") : TEXT("procedural boxes"));
	if (!bReal)
	{
	// nose forward = +X. Units = cm.
	AddBox(Raw, {-150,-70, 40}, {140, 70,170});   // fuselage body
	AddBox(Raw, {120,-55, 45},  {215, 55,130});    // nose / cockpit snout
	AddBox(Raw, {-420,-22,110}, {-140, 22,152});   // tail boom
	AddBox(Raw, {-445,-12,110}, {-405, 12,225});   // tail fin (vertical)
	AddBox(Raw, {-470, 8,150},  {-455, 60,205});   // tail rotor (vertical, side)
	AddBox(Raw, {-20,-12,170},  {20, 12,215});     // main rotor mast
	AddBox(Raw, {-320,-16,213}, {320, 16,225});    // main rotor blade (X)
	AddBox(Raw, {-16,-320,213}, {16, 320,225});    // main rotor blade (Y)
	AddBox(Raw, {-120,-92, 0},  {120,-72, 14});    // left skid
	AddBox(Raw, {-120, 72, 0},  {120, 92, 14});    // right skid
	AddBox(Raw, {70,-88,12},    {88,-74, 42});      // strut LF
	AddBox(Raw, {-100,-88,12},  {-82,-74, 42});     // strut LR
	AddBox(Raw, {70, 74,12},    {88, 88, 42});      // strut RF
	AddBox(Raw, {-100, 74,12},  {-82, 88, 42});     // strut RR
	}

	const FString PkgName = TEXT("/Game/TurdMOD/SM_TurdHeli");
	UPackage* Pkg = CreatePackage(*PkgName);
	Pkg->FullyLoad();
	UStaticMesh* Mesh = NewObject<UStaticMesh>(Pkg, FName(TEXT("SM_TurdHeli")), RF_Public | RF_Standalone);
	Mesh->AddSourceModel();
	FStaticMeshSourceModel& Src = Mesh->GetSourceModel(0);
	Src.BuildSettings.bRecomputeNormals = true;
	Src.BuildSettings.bRecomputeTangents = true;
	Src.BuildSettings.bRemoveDegenerates = true;
	Src.RawMeshBulkData->SaveRawMesh(Raw);
	Mesh->StaticMaterials.Add(FStaticMaterial(Mat, FName(TEXT("Mat0")), FName(TEXT("Mat0"))));
	Mesh->Build(false);
	Mesh->PostEditChange();
	Pkg->MarkPackageDirty();
	const FString Fn = FPackageName::LongPackageNameToFilename(PkgName, FPackageName::GetAssetPackageExtension());
	const bool bSaved = UPackage::SavePackage(Pkg, Mesh, RF_Public | RF_Standalone, *Fn);

	// Cook map: place the heli mesh in a UWorld so the cook pulls SM_TurdHeli +
	// M_TurdHeli (and compiles the mesh's shaders) — same trick as the livery map.
	bool bMap = false;
	{
		const FString MapPkgName = TEXT("/Game/TurdMOD/M_HeliCook");
		UPackage* MapPkg = CreatePackage(*MapPkgName);
		MapPkg->SetPackageFlags(PKG_ContainsMap);
		UWorld* World = NewObject<UWorld>(MapPkg, FName(TEXT("M_HeliCook")), RF_Public | RF_Standalone);
		World->WorldType = EWorldType::Inactive;
		World->InitializeNewWorld(UWorld::InitializationValues()
			.CreatePhysicsScene(false).ShouldSimulatePhysics(false)
			.EnableTraceCollision(false).CreateNavigation(false).CreateAISystem(false));
		if (AStaticMeshActor* A = World->SpawnActor<AStaticMeshActor>())
		{
			if (A->GetStaticMeshComponent()) { A->GetStaticMeshComponent()->SetStaticMesh(Mesh); }
		}
		FAssetRegistryModule::AssetCreated(World);
		MapPkg->MarkPackageDirty();
		const FString MapFn = FPackageName::LongPackageNameToFilename(MapPkgName, FPackageName::GetMapPackageExtension());
		bMap = UPackage::SavePackage(MapPkg, World, RF_Public | RF_Standalone, *MapFn);
	}

	UE_LOG(LogTemp, Warning, TEXT("[TurdHeli] === DONE: SM_TurdHeli saved=%d verts=%d tris=%d mat=%d map=%d ==="),
		bSaved ? 1 : 0, Raw.VertexPositions.Num(), Raw.WedgeIndices.Num() / 3, Mat ? 1 : 0, bMap ? 1 : 0);
	return (bSaved && bMap) ? 0 : 3;
#else
	UE_LOG(LogTemp, Error, TEXT("[TurdHeli] requires WITH_EDITOR"));
	return 1;
#endif
}
