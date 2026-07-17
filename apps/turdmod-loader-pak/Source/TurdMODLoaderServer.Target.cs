using UnrealBuildTool;
using System.Collections.Generic;

public class TurdMODLoaderServerTarget : TargetRules
{
	public TurdMODLoaderServerTarget(TargetInfo Target) : base(Target)
	{
		Type = TargetType.Server;
		DefaultBuildSettings = BuildSettingsVersion.V2;
		ExtraModuleNames.AddRange(new string[] { "TurdMODLoader" });
	}
}
