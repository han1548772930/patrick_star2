slint::slint! {
    export component AppTray inherits SystemTrayIcon {
        icon: @image-url("../../assets/icons/emotion.svg");
        title: "Patrick Star";
        tooltip: "Patrick Star";

        Menu {
            MenuItem {
                title: "Capture";
                activated => { root.capture(); }
            }
            MenuItem {
                title: "Settings";
                activated => { root.settings(); }
            }
            MenuSeparator { }
            MenuItem {
                title: "Quit";
                activated => { root.quit(); }
            }
        }

        clicked => { root.capture(); }

        callback capture();
        callback settings();
        callback quit();
    }
}
