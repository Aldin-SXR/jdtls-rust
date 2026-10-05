package p;

import java.io.ByteArrayInputStream;
import java.io.IOException;

public class A {
    void run() {
        try (ByteArrayInputStream in = new ByteArrayInputStream(new byte[0])) {
            System.out.println(in.read());
        } catch (IOException ex) {
            ex.printStackTrace();
        }
    }
}
