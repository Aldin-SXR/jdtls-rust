package p;

import java.io.ByteArrayInputStream;
import java.io.IOException;

public class A {
    void read() throws IOException {}
    void run() {
        try (ByteArrayInputStream byteArrayInputStream = new java.io.ByteArrayInputStream(new byte[0])) {
            read();
        } catch (IOException ex) {
            ex.printStackTrace();
        }
    }
}
