use crate::post_login::PostLoginEnvironment;

#[test]
fn test_wayland_serialization() {
    let env = PostLoginEnvironment::Wayland {
        script_path: "/bin/sh".to_string(),
        env_name: "Hyprland".to_string(),
    };

    assert_eq!(env.to_xdg_type(), "wayland");
    assert_eq!(env.to_xdg_desktop(), Some("Hyprland"));
}

#[test]
fn test_shell_serialization() {
    let env = PostLoginEnvironment::Shell;

    assert_eq!(env.to_xdg_type(), "tty");
    assert_eq!(env.to_xdg_desktop(), None);
}
