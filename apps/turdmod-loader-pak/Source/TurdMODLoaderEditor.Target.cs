using UnrealBuildTool;
using System.Collections.Generic;

public class TurdMODLoaderEditorTarget : TargetRules
{
	public TurdMODLoaderEditorTarget(TargetInfo Target) : base(Target)
	{
		Type = TargetType.Editor;
		DefaultBuildSettings = BuildSettingsVersion.V2;
		ExtraModuleNames.AddRange(new string[] { "TurdMODLoader" });
	}
}
