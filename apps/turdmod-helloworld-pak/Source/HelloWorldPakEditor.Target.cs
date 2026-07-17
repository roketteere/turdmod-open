// HelloWorldPakEditor.Target.cs — editor target. Used when building
// the UE4 Editor for this project. Mirrors the game target with the
// Editor type so the Editor knows which modules to load.

using UnrealBuildTool;
using System.Collections.Generic;

public class HelloWorldPakEditorTarget : TargetRules
{
	public HelloWorldPakEditorTarget(TargetInfo Target) : base(Target)
	{
		Type = TargetType.Editor;
		DefaultBuildSettings = BuildSettingsVersion.V2;
		// SCUM = stub module providing /Script/SCUM.Airplane (+ parent chain) so BPs parent to it.
		ExtraModuleNames.AddRange(new string[] { "HelloWorldPak", "SCUM" });
	}
}
