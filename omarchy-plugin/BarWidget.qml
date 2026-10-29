import QtQuick
import Quickshell
import qs.Commons
import qs.Commons as Commons
import qs.Ui

BarWidget {
  id: root
  moduleName: "local.display-layout-editor"

  Commons.Translations {
    id: strings
    directory: Qt.resolvedUrl("i18n")
  }

  readonly property string launcher:
    Qt.resolvedUrl("bin/launch-display-layout-editor").toString().replace(/^file:\/\//, "")

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: "󰍺"
    tooltipText: strings.t("bar.tooltip")
    onPressed: root.bar.run(Util.shellQuote(root.launcher))
  }
}
