slint::slint! {
    import { TextEdit } from "std-widgets.slint";

    component VectorButton inherits Rectangle {
        in property <string> icon-data;
        in property <string> label;
        in property <bool> active: false;
        in property <bool> enabled: true;
        callback clicked;

        width: 30px;
        height: 30px;
        border-radius: 4px;
        background: !root.enabled ? transparent
            : root.active ? #dcecff
            : touch.has-hover ? #343a42
            : transparent;

        Path {
            x: 6px;
            y: 6px;
            width: 18px;
            height: 18px;
            viewbox-width: 24;
            viewbox-height: 24;
            commands: root.icon-data;
            fill: transparent;
            stroke: !root.enabled ? #666c74 : root.active ? #1263a0 : #e4e7eb;
            stroke-width: 1.5px;
        }

        touch := TouchArea {
            enabled: root.enabled;
            mouse-cursor: pointer;
            accessible-role: button;
            accessible-label: root.label;
            clicked => { root.clicked(); }
        }
    }

    component IconButton inherits Rectangle {
        in property <image> icon;
        in property <string> label;
        in property <bool> active: false;
        in property <bool> enabled: true;
        callback clicked;

        width: 30px;
        height: 30px;
        border-radius: 4px;
        background: !root.enabled ? transparent
            : root.active ? #dcecff
            : touch.has-hover ? #343a42
            : transparent;

        Image {
            x: 6px;
            y: 6px;
            width: 18px;
            height: 18px;
            source: root.icon;
            image-fit: contain;
            colorize: !root.enabled ? #666c74 : root.active ? #1263a0 : #e4e7eb;
        }

        touch := TouchArea {
            enabled: root.enabled;
            mouse-cursor: pointer;
            accessible-role: button;
            accessible-label: root.label;
            clicked => { root.clicked(); }
        }
    }

    component WindowButton inherits Rectangle {
        in property <string> icon-data;
        in property <string> label;
        in property <bool> destructive: false;
        callback clicked;

        width: 48px;
        height: 30px;
        background: touch.has-hover
            ? root.destructive ? #c42b1c : #343a42
            : transparent;

        Path {
            x: 16px;
            y: 7px;
            width: 16px;
            height: 16px;
            viewbox-width: 24;
            viewbox-height: 24;
            commands: root.icon-data;
            fill: transparent;
            stroke: #eef0f2;
            stroke-width: 1.4px;
        }

        touch := TouchArea {
            mouse-cursor: pointer;
            accessible-role: button;
            accessible-label: root.label;
            clicked => { root.clicked(); }
        }
    }

    export component PreviewWindow inherits Window {
        preferred-width: 960px;
        preferred-height: 680px;
        min-width: 560px;
        min-height: 200px;
        no-frame: true;
        background: transparent;
        title: root.preview-title;
        default-font-size: 13px;

        in property <string> preview-title: "Patrick Star";
        in property <bool> ocr-panel-visible: false;
        in property <string> ocr-text;
        in property <int> active-tool: 0;
        in property <bool> pan-active: false;
        in property <bool> can-undo: false;
        in property <bool> can-redo: false;
        in property <int> zoom-percent: 100;

        out property <length> canvas-width: canvas.width;
        out property <length> canvas-height: canvas.height;
        out property <length> canvas-x: canvas.x;
        out property <length> canvas-y: canvas.y;
        private property <length> ocr-panel-width: root.ocr-panel-visible
            ? min(360px, max(280px, root.width * 0.3))
            : 0px;

        callback choose-tool(int);
        callback choose-pan;
        callback undo;
        callback redo;
        callback zoom-in;
        callback zoom-out;
        callback actual-size;
        callback fit;
        callback rotate;
        callback copy;
        callback save;
        callback minimize;
        callback toggle-maximize;
        callback request-close;
        callback canvas-press(float, float);
        callback canvas-double-click(float, float);
        callback canvas-move(float, float);
        callback canvas-release;
        callback canvas-scroll(float, float, float);
        callback key-input(string, bool, bool, bool) -> bool;

        Rectangle {
            width: parent.width;
            height: parent.height;
            background: transparent;

            titlebar := Rectangle {
                width: parent.width;
                height: 60px;
                background: #202328;

                HorizontalLayout {
                    x: 8px;
                    width: parent.width - 152px;
                    height: 30px;
                    spacing: 2px;
                    alignment: start;

                    IconButton { icon: @image-url("../../assets/icons/zoom-in.svg"); label: "Zoom in"; clicked => { root.zoom-in(); } }
                    IconButton { icon: @image-url("../../assets/icons/zoom-out.svg"); label: "Zoom out"; clicked => { root.zoom-out(); } }
                    IconButton { icon: @image-url("../../assets/icons/actual-size.svg"); label: "Actual size"; clicked => { root.actual-size(); } }
                    IconButton { icon: @image-url("../../assets/icons/fit-to-window.svg"); label: "Fit to window"; clicked => { root.fit(); } }
                    IconButton { icon: @image-url("../../assets/icons/rotate-left.svg"); label: "Rotate"; clicked => { root.rotate(); } }

                    Rectangle { width: 1px; height: 20px; background: #454a52; }

                    IconButton { icon: @image-url("../../assets/icons/undo-2.svg"); label: "Undo"; enabled: root.can-undo; clicked => { root.undo(); } }
                    VectorButton { icon-data: "M 15 8 L 20 13 L 15 18 M 20 13 L 9 13 C 6 13 4 15 4 18"; label: "Redo"; enabled: root.can-redo; clicked => { root.redo(); } }

                    Text {
                        horizontal-stretch: 1;
                        text: root.preview-title;
                        color: #d7dbe0;
                        font-size: 12px;
                        horizontal-alignment: center;
                        vertical-alignment: center;
                    }

                    VectorButton { icon-data: "M 8 8 L 20 8 L 20 20 L 8 20 Z M 4 16 L 4 4 L 16 4"; label: "Copy"; clicked => { root.copy(); } }
                    IconButton { icon: @image-url("../../assets/icons/download.svg"); label: "Save"; clicked => { root.save(); } }
                }

                HorizontalLayout {
                    x: parent.width - 144px;
                    width: 144px;
                    height: 30px;
                    spacing: 0;

                    WindowButton { icon-data: "M 5 12 L 19 12"; label: "Minimize"; clicked => { root.minimize(); } }
                    WindowButton { icon-data: "M 6 6 L 18 6 L 18 18 L 6 18 Z"; label: "Maximize or restore"; clicked => { root.toggle-maximize(); } }
                    WindowButton { icon-data: "M 6 6 L 18 18 M 18 6 L 6 18"; label: "Close"; destructive: true; clicked => { root.request-close(); } }
                }

                Rectangle {
                    y: 30px;
                    width: parent.width;
                    height: 30px;
                    background: #262a30;

                    HorizontalLayout {
                        x: 8px;
                        width: parent.width - 16px;
                        height: 30px;
                        spacing: 2px;
                        alignment: start;

                        IconButton { icon: @image-url("../../assets/icons/edit.svg"); label: "Select"; active: root.active-tool == 0 && !root.pan-active; clicked => { root.choose-tool(0); } }
                        IconButton { icon: @image-url("../../assets/icons/square.svg"); label: "Rectangle"; active: root.active-tool == 1 && !root.pan-active; clicked => { root.choose-tool(1); } }
                        IconButton { icon: @image-url("../../assets/icons/circle.svg"); label: "Ellipse"; active: root.active-tool == 2 && !root.pan-active; clicked => { root.choose-tool(2); } }
                        IconButton { icon: @image-url("../../assets/icons/move-up-right.svg"); label: "Arrow"; active: root.active-tool == 4 && !root.pan-active; clicked => { root.choose-tool(4); } }
                        IconButton { icon: @image-url("../../assets/icons/pen.svg"); label: "Pen"; active: root.active-tool == 5 && !root.pan-active; clicked => { root.choose-tool(5); } }
                        IconButton { icon: @image-url("../../assets/icons/mosaic.svg"); label: "Mosaic"; active: root.active-tool == 6 && !root.pan-active; clicked => { root.choose-tool(6); } }
                        IconButton { icon: @image-url("../../assets/icons/type.svg"); label: "Text"; active: root.active-tool == 7 && !root.pan-active; clicked => { root.choose-tool(7); } }
                        IconButton { icon: @image-url("../../assets/icons/emotion.svg"); label: "Emotion"; active: root.active-tool == 3 && !root.pan-active; clicked => { root.choose-tool(3); } }

                        Rectangle { width: 1px; height: 20px; background: #454a52; }

                        VectorButton { icon-data: "M 8 12 L 4 12 L 4 8 M 4 12 C 6 6 15 5 19 10 M 16 12 L 20 12 L 20 16 M 20 12 C 18 18 9 19 5 14"; label: "Pan"; active: root.pan-active; clicked => { root.choose-pan(); } }
                    }
                }
            }

            canvas := Rectangle {
                y: 60px;
                width: parent.width - root.ocr-panel-width;
                height: max(1px, parent.height - 88px);
                background: transparent;

                canvas-touch := TouchArea {
                    mouse-cursor: root.pan-active ? grab : crosshair;
                    pointer-event(event) => {
                        if event.button != PointerEventButton.left {
                            return;
                        }
                        if event.kind == PointerEventKind.down {
                            key-scope.focus();
                            root.canvas-press(self.mouse-x / 1px, self.mouse-y / 1px);
                        } else if event.kind == PointerEventKind.up || event.kind == PointerEventKind.cancel {
                            root.canvas-release();
                        }
                    }
                    moved => { root.canvas-move(self.mouse-x / 1px, self.mouse-y / 1px); }
                    double-clicked => {
                        key-scope.focus();
                        root.canvas-double-click(self.mouse-x / 1px, self.mouse-y / 1px);
                    }
                    scroll-event(event) => {
                        root.canvas-scroll(self.mouse-x / 1px, self.mouse-y / 1px, event.delta-y / 1px);
                        accept
                    }
                }
            }

            if root.ocr-panel-visible: Rectangle {
                x: parent.width - root.ocr-panel-width;
                y: 60px;
                width: root.ocr-panel-width;
                height: max(1px, parent.height - 88px);
                background: #f4f5f7;

                Rectangle {
                    width: 1px;
                    height: parent.height;
                    background: #d6d9de;
                }

                TextEdit {
                    x: 12px;
                    y: 12px;
                    width: parent.width - 24px;
                    height: parent.height - 24px;
                    text: root.ocr-text;
                    read-only: true;
                    wrap: word-wrap;
                    font-size: 15px;
                    accessible-label: "OCR result";
                }
            }

            Rectangle {
                y: parent.height - 28px;
                width: parent.width;
                height: 28px;
                background: #202328;

                Text {
                    x: canvas.width - 92px;
                    width: 80px;
                    height: parent.height;
                    horizontal-alignment: right;
                    vertical-alignment: center;
                    color: #b9bec5;
                    text: root.zoom-percent + "%";
                }
            }

            key-scope := FocusScope {
                width: 0;
                height: 0;
                key-pressed(event) => {
                    root.key-input(event.text, event.modifiers.control, event.modifiers.shift, event.modifiers.alt)
                        ? accept : reject
                }
            }
        }
    }
}
