// HelloWorldPak.Target.cs — game target. Used when building the
// standalone (non-Editor) game executable. We don't actually ship a
// standalone .exe — the cooked content goes into a .pak that mounts
// on GameServer.exe — but UBT requires this file to exist for the
// Editor build to succeed.

using UnrealBuildTool;
using System.Collections.Generic;

public class HelloWorldPakTarget : TargetRules
{
	public HelloWorldPakTarget(TargetInfo Target) : base(Target)
	{
		Type = TargetType.Game;
		DefaultBuildSettings = BuildSettingsVersion.V2;
		ExtraModuleNames.AddRange(new string[] { "HelloWorldPak" });
	}
}
