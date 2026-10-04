package com.jdtls.ecjbridge;

import java.util.*;

import org.eclipse.jdt.core.formatter.CodeFormatter;
import org.eclipse.jdt.core.formatter.DefaultCodeFormatterConstants;
import org.eclipse.jdt.internal.formatter.DefaultCodeFormatter;
import org.eclipse.text.edits.InsertEdit;
import org.eclipse.text.edits.ReplaceEdit;
import org.eclipse.text.edits.TextEdit;

import com.jdtls.ecjbridge.BridgeProtocol.*;

/**
 * Runs the Eclipse code formatter.  The Rust server resolves the complete
 * option map (defaults, profile, project and client options) and computes
 * the region; this only calls {@code CodeFormatter.format} and returns the
 * flattened leaf edits (jdt.ls {@code TextEditUtil.flatten}).
 */
public class FormatterService {

    /**
     * @return the leaf edits, or {@code null} when the formatter returned {@code null}
     */
    public List<BridgeFormatEdit> format(String source, int kind, int offset, int length,
                                         int indentationLevel, String lineSeparator,
                                         Map<String, String> options) {
        CodeFormatter formatter = createCodeFormatter(options);
        TextEdit edit = formatter.format(kind, source, offset, length, indentationLevel, lineSeparator);
        if (edit == null) {
            return null;
        }
        List<BridgeFormatEdit> out = new ArrayList<>();
        // jdt.ls treats a root without children as "no edits".
        for (TextEdit child : edit.getChildren()) {
            flatten(child, out);
        }
        return out;
    }

    /**
     * {@code ToolFactory.createCodeFormatter(options)} ({@code M_FORMAT_EXISTING},
     * as {@code ASTRewriteFormatter} uses it): the options as given.
     */
    public List<BridgeFormatEdit> formatExisting(String source, int kind, int offset, int length,
                                                 int indentationLevel, String lineSeparator,
                                                 Map<String, String> options) {
        CodeFormatter formatter = new DefaultCodeFormatter(new HashMap<>(options == null ? Map.of() : options));
        TextEdit edit = formatter.format(kind, source, offset, length, indentationLevel, lineSeparator);
        if (edit == null) {
            return null;
        }
        List<BridgeFormatEdit> out = new ArrayList<>();
        for (TextEdit child : edit.getChildren()) {
            flatten(child, out);
        }
        return out;
    }

    /**
     * {@code ToolFactory.createCodeFormatter(options, M_FORMAT_NEW)} without the
     * extension-point lookup (no OSGi here).
     */
    private static CodeFormatter createCodeFormatter(Map<String, String> options) {
        Map<String, String> current = new HashMap<>(options == null ? Map.of() : options);
        current.put(DefaultCodeFormatterConstants.FORMATTER_COMMENT_FORMAT_LINE_COMMENT_STARTING_ON_FIRST_COLUMN, DefaultCodeFormatterConstants.TRUE);
        current.put(DefaultCodeFormatterConstants.FORMATTER_NEVER_INDENT_BLOCK_COMMENTS_ON_FIRST_COLUMN, DefaultCodeFormatterConstants.FALSE);
        current.put(DefaultCodeFormatterConstants.FORMATTER_NEVER_INDENT_LINE_COMMENTS_ON_FIRST_COLUMN, DefaultCodeFormatterConstants.FALSE);
        return new DefaultCodeFormatter(current);
    }

    private static void flatten(TextEdit edit, List<BridgeFormatEdit> out) {
        if (!edit.hasChildren()) {
            BridgeFormatEdit e = new BridgeFormatEdit();
            e.offset = edit.getOffset();
            e.length = edit.getLength();
            if (edit instanceof ReplaceEdit r) {
                e.text = r.getText();
            } else if (edit instanceof InsertEdit i) {
                e.text = i.getText();
            } else {
                e.text = "";
            }
            out.add(e);
            return;
        }
        for (TextEdit child : edit.getChildren()) {
            flatten(child, out);
        }
    }
}
