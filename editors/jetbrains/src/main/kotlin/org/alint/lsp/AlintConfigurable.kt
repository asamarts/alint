package org.alint.lsp

import com.intellij.openapi.fileChooser.FileChooser
import com.intellij.openapi.fileChooser.FileChooserDescriptor
import com.intellij.openapi.options.Configurable
import com.intellij.openapi.project.ProjectManager
import com.intellij.openapi.ui.TextFieldWithBrowseButton
import com.intellij.ui.components.JBLabel
import java.awt.BorderLayout
import java.awt.GridBagConstraints
import java.awt.GridBagLayout
import java.awt.Insets
import javax.swing.JComponent
import javax.swing.JPanel

/**
 * Settings UI under **Settings -> Tools -> alint** exposing `AlintSettings.alintPath`.
 */
class AlintConfigurable : Configurable {
    private var pathField: TextFieldWithBrowseButton? = null

    override fun getDisplayName(): String = "alint"

    override fun createComponent(): JComponent {
        val field = TextFieldWithBrowseButton()
        field.addActionListener {
            val descriptor = FileChooserDescriptor(true, false, false, false, false, false)
                .withTitle("Select alint Binary")
                .withDescription("Choose the alint executable to run `alint lsp`.")
            val chosen = FileChooser.chooseFile(descriptor, null, null)
            if (chosen != null) {
                field.text = chosen.path
            }
        }
        pathField = field
        reset()

        val form = JPanel(GridBagLayout())
        val gbc = GridBagConstraints().apply {
            insets = Insets(4, 4, 4, 4)
            anchor = GridBagConstraints.WEST
            gridy = 0
        }
        gbc.gridx = 0
        gbc.weightx = 0.0
        gbc.fill = GridBagConstraints.NONE
        form.add(JBLabel("alint binary path:"), gbc)

        gbc.gridx = 1
        gbc.weightx = 1.0
        gbc.fill = GridBagConstraints.HORIZONTAL
        form.add(field, gbc)

        gbc.gridx = 1
        gbc.gridy = 1
        gbc.weightx = 1.0
        form.add(
            JBLabel("Leave empty to use `alint` on PATH or the managed download."),
            gbc,
        )

        val wrapper = JPanel(BorderLayout())
        wrapper.add(form, BorderLayout.NORTH)
        return wrapper
    }

    override fun isModified(): Boolean {
        val current = pathField?.text?.trim() ?: return false
        return current != AlintSettings.getInstance().alintPath.trim()
    }

    override fun apply() {
        val next = pathField?.text?.trim() ?: return
        val settings = AlintSettings.getInstance()
        val previous = settings.alintPath.trim()
        settings.alintPath = next
        if (next != previous) {
            for (project in ProjectManager.getInstance().openProjects) {
                if (!project.isDisposed && !project.isDefault) {
                    AlintNotifier.resetMissingNotice(project)
                    AlintTrustCoordinator.onSettingsChanged(project)
                }
            }
        }
    }

    override fun reset() {
        pathField?.text = AlintSettings.getInstance().alintPath
    }

    override fun disposeUIResources() {
        pathField = null
    }
}
