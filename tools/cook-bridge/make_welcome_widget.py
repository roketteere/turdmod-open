import unreal
PKG="/Game/TurdMOD"; NAME="WBP_TurdMODWelcome"
tools=unreal.AssetToolsHelpers.get_asset_tools()
wbp=tools.create_asset(NAME, PKG, unreal.WidgetBlueprint, unreal.WidgetBlueprintFactory())
unreal.log_warning("create_asset -> %s" % (wbp.get_path_name() if wbp else "NONE"))
saved=False
# commandlet-safe save attempts (EditorAssetLibrary was unavailable in -run mode)
for label, fn in [
    ("EditorLoadingAndSavingUtils.save_packages", lambda: unreal.EditorLoadingAndSavingUtils.save_packages([wbp.get_outermost()], False)),
    ("EditorAssetLibrary.save_asset", lambda: unreal.EditorAssetLibrary.save_asset(wbp.get_path_name(), False)),
]:
    try:
        fn(); saved=True; unreal.log_warning("SAVED via %s" % label); break
    except Exception as e:
        unreal.log_warning("save miss (%s): %s" % (label, e))
unreal.log_warning("=== WIDGET RESULT: created=%s saved=%s ===" % (wbp is not None, saved))
