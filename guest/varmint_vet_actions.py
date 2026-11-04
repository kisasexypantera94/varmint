"""Desktop actions shared by the Vet UI."""

from pathlib import Path
import subprocess

from varmint_vet import (
    VetError,
    display_proton,
    transform_launch_options,
    venus_supports_proton,
)


NOT_SET = object()
NOTIFICATION_TIMEOUT_MS = 10000


def notification_icon():
    for path in (
        Path.home() / ".local/share/icons/hicolor/512x512/apps/varmint-vet.png",
        Path("/usr/local/share/icons/hicolor/512x512/apps/varmint-vet.png"),
    ):
        if path.is_file():
            return str(path)
    return "dialog-information"


def copy_to_clipboard(text):
    for arguments in (["wl-copy"], ["xclip", "-selection", "clipboard", "-in"]):
        try:
            result = subprocess.run(
                arguments,
                input=text,
                text=True,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                timeout=5,
                check=False,
            )
        except (FileNotFoundError, subprocess.TimeoutExpired):
            continue
        if result.returncode == 0:
            return
    raise VetError("clipboard_failed", "Vet could not copy the text to the clipboard.")


def open_game_properties(appid):
    try:
        subprocess.Popen(
            ["steam", f"steam://gameproperties/{appid}"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
    except OSError as exception:
        raise VetError(
            "steam_open_failed", "Vet could not open the game's Steam properties."
        ) from exception


def notify(title, message):
    try:
        subprocess.run(
            [
                "notify-send",
                "--app-name=Vet",
                "--urgency=normal",
                "--icon",
                notification_icon(),
                f"--expire-time={NOTIFICATION_TIMEOUT_MS}",
                title,
                message,
            ],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        )
    except OSError:
        pass


def configure_manually(plan):
    instructions = []
    if plan["changes"]["launch_options"]:
        copy_to_clipboard(plan["after"]["launch_options"])
        instructions.append("Paste into General → Launch Options.")
    if plan["changes"]["proton"]:
        proton = plan["after"]["proton"]
        instructions.append(
            f"In Compatibility, select {display_proton(proton)}."
            if proton
            else "In Compatibility, disable the per-game override."
        )
    if not instructions:
        return False
    open_game_properties(plan["appid"])
    notify(
        "Launch Options copied"
        if plan["changes"]["launch_options"]
        else "Steam Properties opened",
        " ".join(instructions),
    )
    return True


def override_plan(state, launch_options=NOT_SET, proton=NOT_SET):
    target_launch = (
        state["launch_options"] if launch_options is NOT_SET else launch_options
    )
    target_proton = state["proton"] if proton is NOT_SET else proton
    return {
        "appid": state["appid"],
        "before": {
            "launch_options": state["launch_options"],
            "proton": state["proton"],
        },
        "after": {"launch_options": target_launch, "proton": target_proton},
        "changes": {
            "launch_options": target_launch != state["launch_options"],
            "proton": target_proton != state["proton"],
        },
    }


def switch_backend_plan(state, mode):
    launch_options = NOT_SET
    if mode != state["effective_graphics_mode"]:
        launch_options = transform_launch_options(state["launch_options"], mode)
    proton = NOT_SET
    if mode == "venus" and not venus_supports_proton(state["effective_proton"]):
        proton = "proton_10"
    return override_plan(state, launch_options=launch_options, proton=proton)
