// SCUM.Build.cs — a STUB module deliberately named "SCUM" so its class package path is
// `/Script/SCUM`. It declares the minimal vehicle parent chain (AAirplane → … → APawn) from
// the dumped SCUM SDK, JUST enough for the editor to author a Blueprint parented to
// `/Script/SCUM.Airplane`. In-game, SCUM's REAL `/Script/SCUM` module provides these classes
// (identical paths) — our stubs are editor/cook-only and carry NO properties.
//
// ⚠ Do NOT add gameplay deps here. This compiles into the editor + content cook only; it is
// never meant to be a real runtime. Keep it tiny so the names/paths line up and nothing else.
using UnrealBuildTool;

public class SCUM : ModuleRules
{
	public SCUM(ReadOnlyTargetRules Target) : base(Target)
	{
		PCHUsage = PCHUsageMode.UseExplicitOrSharedPCHs;

		// Expose this module's headers (SCUMVehicleStubs.h) to dependents (HelloWorldPak commandlets).
		PublicIncludePaths.Add(ModuleDirectory);

		PublicDependencyModuleNames.AddRange(new string[]
		{
			"Core",
			"CoreUObject",
			"Engine", // APawn
		});
	}
}
