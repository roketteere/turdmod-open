using UnrealBuildTool;
using System.Collections.Generic;

public class TurdMODLoaderTarget : TargetRules
{
	public TurdMODLoaderTarget(TargetInfo Target) : base(Target)
	{
		Type = TargetType.Game;
		DefaultBuildSettings = BuildSettingsVersion.V2;
		ExtraModuleNames.AddRange(new string[] { "TurdMODLoader" });
	}
}
