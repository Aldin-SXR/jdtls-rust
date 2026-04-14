package com.jdtls.ecjbridge;

import com.google.googlejavaformat.java.Formatter;
import com.google.googlejavaformat.java.FormatterException;

import java.util.*;
import java.util.logging.Logger;

import com.jdtls.ecjbridge.BridgeProtocol.*;

/**
 * Formats Java source using google-java-format.
 */
public class FormatterService {

    private static final Logger LOG = Logger.getLogger(FormatterService.class.getName());

    // google-java-format Formatter instance (thread-safe)
    private final Formatter formatter;
    private final boolean available;

    public FormatterService() {
        Formatter f = null;
        boolean ok = false;
        try {
            f = new Formatter();
            ok = true;
        } catch (Throwable t) {
            LOG.warning("google-java-format not available: " + t.getMessage());
        }
        this.formatter = f;
        this.available = ok;
    }

    public List<BridgeTextEdit> format(String source, int tabSize, boolean insertSpaces) {
        if (!available || formatter == null) {
            return Collections.emptyList();
        }
        try {
            String formatted = formatter.formatSource(source);
            if (formatted.equals(source)) return Collections.emptyList();

            // Return single full-file replacement
            BridgeTextEdit te = new BridgeTextEdit();
            te.startLine = 0;
            te.startChar = 0;
            String[] lines = source.split("\n", -1);
            te.endLine = lines.length - 1;
            te.endChar = lines[lines.length - 1].length();
            te.newText = formatted;
            return List.of(te);
        } catch (FormatterException e) {
            LOG.warning("Formatter error: " + e.getMessage());
            return Collections.emptyList();
        }
    }
}
