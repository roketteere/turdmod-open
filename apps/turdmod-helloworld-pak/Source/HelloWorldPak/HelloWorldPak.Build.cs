// HelloWorldPak.Build.cs — UBT module config for the Phase C
// helloworld pak. Minimum-viable C++ module; the actual content
// (BP_HelloWorld + its BroadcastHelloWorld UFunction) is authored
// in the Editor and ships as a cooked .uasset inside the pak.
//
// Dependencies are just the UE4 stock set. We do NOT pull in any
// SCUM headers — the BP subclasses SCUM classes via Editor
// asset-borrowing (Dumper-7 uassets dropped into Content/SCUM/),
// not via C++ inheritance.

using UnrealBuildTool;

public class HelloWorldPak : ModuleRules
{
	public HelloWorldPak(ReadOnlyTargetRules Target) : base(Target)
	{
		PCHUsage = PCHUsageMode.UseExplicitOrSharedPCHs;

		PublicDependencyModuleNames.AddRange(new string[]
		{
			"Core",
			"CoreUObject",
			"Engine",
			"SCUM",  // stub UVehiclePreset/UVehiclePresetNode for the preset-author commandlet
		});

		PrivateDependencyModuleNames.AddRange(new string[] { });

		// EDITOR-ONLY: the headless asset-authoring commandlets (TurdMODAuthorWidget,
		// etc.) need the UMG runtime + editor + Kismet compiler. Gated on bBuildEditor
		// so the GAME/SERVER cook target never tries to link UnrealEd (absent in
		// packaged builds). @brk drop this guard and the server cook fails to link.
		if (Target.bBuildEditor)
		{
			PrivateDependencyModuleNames.AddRange(new string[]
			{
				"UMG",            // UWidgetTree, UUserWidget, UTextBlock, UCanvasPanel...
				"UMGEditor",      // UWidgetBlueprint
				"UnrealEd",       // UCommandlet host, asset save/edit
				"Kismet",         // editor blueprint helpers
				"KismetCompiler", // FKismetEditorUtilities::CompileBlueprint
				"AssetRegistry",
				"AssetTools",     // FAssetToolsModule / IAssetTools — texture import
				"RawMesh",        // FRawMesh — procedural static-mesh authoring (heli)
				"MeshUtilities",  // UStaticMesh::Build()
				"MeshDescription",
				"StaticMeshDescription",
				"Slate",
				"SlateCore",
			});
		}
	}
}
